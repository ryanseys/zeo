//! The `Encoding` class (CRuby encoding.c) -- the Ruby-visible face of the
//! `encoding` engine. An `Encoding` value wraps an [`EncodingId`] and there
//! is exactly ONE per encoding (`Encoding::UTF_8.equal?(s.encoding)` holds),
//! so the objects are interned once in `SINGLETONS`.
//!
//! Instance methods (`#name`/`#names`/`#inspect`/`#==`) reach here through
//! the ordinary MRO walk on an `Encoding` receiver; the module-level queries
//! (`Encoding.list`/`.find`/`.compatible?`/`.default_external`) are class
//! methods dispatched on the `Encoding` class value itself.

use std::sync::{Arc, LazyLock};

use crate::builtins::{arity, builtin_methods};
use crate::dispatch::{RObj, RubyObject};
use crate::encoding::{self, EncodingId};
use crate::{RubyValue, Signal};
use spinel_abi::{ClassId, ENCODING_CLASS};

pub struct REncoding {
    pub id: EncodingId,
}

impl RubyObject for REncoding {
    fn class_id(&self) -> ClassId {
        ENCODING_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        // Encoding objects are always frozen in Ruby.
        true
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        // Interned: a "copy" is the same singleton.
        SINGLETONS[self.id.0 as usize].clone()
    }
}

/// The one interned `Encoding` object per table row -- so encoding values
/// compare by identity, exactly like CRuby.
static SINGLETONS: LazyLock<Vec<RObj>> = LazyLock::new(|| {
    encoding::all()
        .map(|id| Arc::new(REncoding { id }) as RObj)
        .collect()
});

/// The singleton `Encoding` value for `id` -- the shared way anything
/// (`String#encoding`, the constants, `Encoding.find`) hands one out.
pub fn encoding_value(id: EncodingId) -> RubyValue {
    RubyValue::Object(SINGLETONS[id.0 as usize].clone())
}

/// The encoding CRuby assigns a Symbol or Regexp by default: US-ASCII when
/// every character is ASCII, else the script encoding (UTF-8). These objects
/// carry no independent encoding tag in this runtime, so it's derived on
/// demand.
pub fn computed_encoding_of(text: &str) -> EncodingId {
    if text.is_ascii() {
        encoding::US_ASCII
    } else {
        encoding::UTF_8
    }
}

fn recv_encoding(recv: &RubyValue) -> EncodingId {
    let RubyValue::Object(o) = recv else {
        unreachable!("the Encoding table only dispatches on Encoding receivers")
    };
    o.as_any()
        .downcast_ref::<REncoding>()
        .expect("class_id guarantees this downcast")
        .id
}

/// Resolves the `Encoding.find`/`String#encode` name-or-Encoding argument to
/// an id, raising CRuby's `ArgumentError` for an unknown name.
pub fn arg_encoding(v: &RubyValue) -> Result<EncodingId, Signal> {
    match v {
        RubyValue::Object(o) if o.class_id() == ENCODING_CLASS => Ok(recv_encoding(v)),
        RubyValue::Str(s) => {
            let name = s.lock().to_utf8_lossy().into_owned();
            resolve_name(&name)
        }
        RubyValue::Symbol(s) => resolve_name(&s.name()),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into String",
                crate::builtins::convert_name_of(other)
            ),
        )),
    }
}

/// `Encoding.find`'s name resolution, including the runtime selector names
/// (`"external"`/`"internal"`/`"locale"`/`"filesystem"`).
fn resolve_name(name: &str) -> Result<EncodingId, Signal> {
    match name.to_ascii_lowercase().as_str() {
        "external" | "locale" | "filesystem" => return Ok(encoding::default_external()),
        "internal" => {
            return encoding::default_internal().ok_or_else(|| {
                // Ruby returns nil for a nil default_internal via find, but
                // `Encoding.find("internal")` specifically returns nil -- the
                // caller handles that; here an unset one is "not found".
                crate::dispatch::raise_error("ArgumentError", "unknown encoding name - internal".into())
            })
        }
        _ => {}
    }
    encoding::find(name).ok_or_else(|| {
        crate::dispatch::raise_error("ArgumentError", format!("unknown encoding name - {name}"))
    })
}

builtin_methods! {
    pub(crate) fn lookup;

    "name" | "to_s" => fn name(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(recv_encoding(recv).name().to_string())))
    }
    "inspect" => fn inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(format!("#<Encoding:{}>", recv_encoding(recv).inspect_name()))))
    }
    "names" => fn names(recv, args, _block) {
        arity!(args, 0);
        let names = recv_encoding(recv)
            .names()
            .into_iter()
            .map(|n| RubyValue::Str(crate::string_new(n.to_string())))
            .collect();
        Ok(RubyValue::Array(crate::array_new(names)))
    }
    "ascii_compatible?" => fn ascii_compat(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_encoding(recv).ascii_compatible()))
    }
    "dummy?" => fn dummy(recv, args, _block) {
        arity!(args, 0);
        // None of the built-in encodings are dummy encodings yet.
        let _ = recv;
        Ok(RubyValue::Bool(false))
    }
    "==" | "eql?" => fn eq(recv, args, _block) {
        arity!(args, 1);
        let same = matches!(&args[0], RubyValue::Object(o) if o.class_id() == ENCODING_CLASS)
            && recv_encoding(recv) == recv_encoding(&args[0]);
        Ok(RubyValue::Bool(same))
    }
    "hash" => fn hash(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_encoding(recv).0 as i64))
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "list" => fn list(_recv, args, _block) {
        arity!(args, 0);
        let all = encoding::all().map(encoding_value).collect();
        Ok(RubyValue::Array(crate::array_new(all)))
    }
    "name_list" => fn name_list(_recv, args, _block) {
        arity!(args, 0);
        let names = encoding::all()
            .flat_map(|id| id.names())
            .map(|n| RubyValue::Str(crate::string_new(n.to_string())))
            .collect();
        Ok(RubyValue::Array(crate::array_new(names)))
    }
    "find" => fn find(_recv, args, _block) {
        arity!(args, 1);
        // `Encoding.find("internal")` returns nil when unset rather than raising.
        if let RubyValue::Str(s) = &args[0] {
            if s.lock().to_utf8_lossy().eq_ignore_ascii_case("internal") {
                return Ok(match encoding::default_internal() {
                    Some(id) => encoding_value(id),
                    None => RubyValue::Nil,
                });
            }
        }
        Ok(encoding_value(arg_encoding(&args[0])?))
    }
    "compatible?" => fn compatible(_recv, args, _block) {
        arity!(args, 2);
        Ok(match compat_of(&args[0], &args[1]) {
            Some(id) => encoding_value(id),
            None => RubyValue::Nil,
        })
    }
    "default_external" => fn default_external(_recv, args, _block) {
        arity!(args, 0);
        Ok(encoding_value(encoding::default_external()))
    }
    "default_external=" => fn set_default_external(_recv, args, _block) {
        arity!(args, 1);
        let id = arg_encoding(&args[0])?;
        encoding::set_default_external(id);
        Ok(encoding_value(id))
    }
    "default_internal" => fn default_internal(_recv, args, _block) {
        arity!(args, 0);
        Ok(match encoding::default_internal() {
            Some(id) => encoding_value(id),
            None => RubyValue::Nil,
        })
    }
    "default_internal=" => fn set_default_internal(_recv, args, _block) {
        arity!(args, 1);
        if matches!(args[0], RubyValue::Nil) {
            encoding::set_default_internal(None);
            return Ok(RubyValue::Nil);
        }
        let id = arg_encoding(&args[0])?;
        encoding::set_default_internal(Some(id));
        Ok(encoding_value(id))
    }
}

/// `Encoding.compatible?` for two objects: strings compare through their
/// contents (an ASCII-only string is compatible with any ascii-compatible
/// encoding), and two `Encoding`s are compatible iff equal or one is
/// US-ASCII. Anything else is incompatible (`nil`).
fn compat_of(a: &RubyValue, b: &RubyValue) -> Option<EncodingId> {
    match (a, b) {
        (RubyValue::Str(a), RubyValue::Str(b)) => {
            // Identity first: `Encoding.compatible?(s, s)` passes one
            // `Arc<Mutex<..>>` twice, and holding both guards at once
            // deadlocks (parking_lot's Mutex is not reentrant). A string is
            // trivially compatible with itself.
            if std::sync::Arc::ptr_eq(a, b) {
                return Some(a.lock().encoding());
            }
            let (a, b) = (a.lock(), b.lock());
            if a.encoding() == b.encoding() {
                return Some(a.encoding());
            }
            if b.is_empty() {
                return Some(a.encoding());
            }
            if a.is_empty() {
                return Some(b.encoding());
            }
            if a.encoding().ascii_compatible() && b.encoding().ascii_compatible() {
                if a.ascii_only() {
                    return Some(b.encoding());
                }
                if b.ascii_only() {
                    return Some(a.encoding());
                }
            }
            None
        }
        _ => {
            let (ea, eb) = (arg_encoding(a).ok()?, arg_encoding(b).ok()?);
            if ea == eb || eb == encoding::US_ASCII {
                Some(ea)
            } else if ea == encoding::US_ASCII {
                Some(eb)
            } else {
                None
            }
        }
    }
}

/// Seeds `Encoding::UTF_8`/`US_ASCII`/`ASCII_8BIT`/`BINARY`/`ISO_8859_1`/...
/// into the constant store -- called once from generated `main()` (the
/// compiler resolved `Encoding::UTF_8` to owner `ENCODING_CLASS` at compile
/// time; only the VALUES need seeding here). Constant names mirror CRuby's:
/// the encoding name upcased with `-`/`.` turned into `_`, plus each alias.
pub fn seed_encoding_constants() {
    use crate::const_set;
    let owner = ENCODING_CLASS.0;
    for id in encoding::all() {
        for name in id.names() {
            const_set(owner, &const_name(name), encoding_value(id));
        }
    }
}

/// The Ruby constant spelling of an encoding name: `"ISO-8859-1"` ->
/// `"ISO_8859_1"`, `"UTF-8"` -> `"UTF_8"`.
fn const_name(name: &str) -> String {
    name.chars()
        .map(|c| if c == '-' || c == '.' { '_' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::{ISO_8859_1, US_ASCII, UTF_8};

    #[test]
    fn encoding_values_are_interned() {
        let a = encoding_value(UTF_8);
        let b = encoding_value(UTF_8);
        let (RubyValue::Object(a), RubyValue::Object(b)) = (&a, &b) else {
            panic!()
        };
        assert!(Arc::ptr_eq(a, b));
    }

    #[test]
    fn name_and_inspect() {
        let utf8 = encoding_value(UTF_8);
        assert_eq!(name(&utf8, &[], None).unwrap().to_display_string(), "UTF-8");
        assert_eq!(
            inspect(&utf8, &[], None).unwrap().to_display_string(),
            "#<Encoding:UTF-8>"
        );
    }

    #[test]
    fn find_resolves_aliases() {
        let found = find(&RubyValue::Class(ENCODING_CLASS), &[str_val("BINARY")], None).unwrap();
        assert_eq!(recv_encoding(&found), encoding::ASCII_8BIT);
    }

    #[test]
    fn compatible_ascii_only_across_encodings() {
        // "abc" (US-ASCII) and a UTF-8 string are compatible -> UTF-8.
        let ascii = RubyValue::Str(crate::string_from_bytes(b"abc".to_vec(), US_ASCII));
        let utf = RubyValue::Str(crate::string_new("caf\u{e9}".into()));
        assert_eq!(compat_of(&ascii, &utf), Some(UTF_8));
        // Two different high-byte encodings are incompatible.
        let latin = RubyValue::Str(crate::string_from_bytes(vec![0xE9], ISO_8859_1));
        let broken_utf = RubyValue::Str(crate::string_new("\u{e9}".into()));
        assert_eq!(compat_of(&latin, &broken_utf), None);
    }

    fn str_val(s: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(s.to_string()))
    }
}
