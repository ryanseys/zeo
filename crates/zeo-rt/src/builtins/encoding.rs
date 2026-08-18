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
use zeo_macros::ruby_class;

use crate::builtins::arg_error;
use crate::dispatch::{RObj, RubyObject};
use crate::encoding::{self, EncodingId};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, ENCODING_CLASS};

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
        // NO Symbol arm: `rb_to_encoding` takes an Encoding or a String, and
        // reaches a Symbol only through `rb_check_string_type`, which refuses
        // it. So `force_encoding(:UTF_8)` and `Encoding.find(:UTF_8)` are both
        // `no implicit conversion of Symbol into String` -- a TypeError about
        // the ARGUMENT, not an ArgumentError about the encoding's name
        // (oracle-checked on 4.0.6; a valid symbol name is refused too).
        other => {
            let name = crate::builtins::convert::to_rstr(other)?
                .lock()
                .to_utf8_lossy()
                .into_owned();
            resolve_name(&name)
        }
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
                arg_error!("unknown encoding name - internal")
            });
        }
        _ => {}
    }
    encoding::find(name).ok_or_else(|| arg_error!("unknown encoding name - {name}"))
}

ruby_class! {
    Encoding = zeo_abi::ENCODING_CLASS < zeo_abi::OBJECT_CLASS;

    def self."list"(_recv) {
        let all = encoding::all().map(encoding_value).collect();
        Ok(RubyValue::Array(crate::array_new(all)))
    }
    // Every registered spelling. `internal` is a registered alias SLOT: when
    // `Encoding.default_internal` is unset it points at no row, so no row's
    // `#names` carries it and it has to be added here.
    def self."name_list"(_recv) {
        let mut names: Vec<RubyValue> = encoding::all()
            .flat_map(|id| id.names())
            .map(|n| RubyValue::Str(crate::string_new(n.to_string())))
            .collect();
        if encoding::default_internal().is_none() {
            names.push(RubyValue::Str(crate::string_new("internal".to_string())));
        }
        Ok(RubyValue::Array(crate::array_new(names)))
    }
    def self."find"(_recv, arg) {
        // `Encoding.find("internal")` returns nil when unset rather than raising.
        if let RubyValue::Str(s) = arg
            && s.lock().to_utf8_lossy().eq_ignore_ascii_case("internal") {
                return Ok(match encoding::default_internal() {
                    Some(id) => encoding_value(id),
                    None => RubyValue::Nil,
                });
            }
        Ok(encoding_value(arg_encoding(arg)?))
    }
    def self."compatible?"(_recv, arg1, arg2) {
        Ok(match compat_of(arg1, arg2) {
            Some(id) => encoding_value(id),
            None => RubyValue::Nil,
        })
    }
    // `Encoding.aliases` -- every alternate spelling mapped to its canonical
    // name, which is exactly the table minus each row's own first name.
    def self."aliases"(_recv) {
        let mut pairs = Vec::new();
        for id in encoding::all() {
            let names = id.names();
            let Some((canonical, aliases)) = names.split_first() else { continue };
            for a in aliases {
                pairs.push((
                    RubyValue::Str(crate::string_new((*a).to_string())),
                    RubyValue::Str(crate::string_new((*canonical).to_string())),
                ));
            }
        }
        Ok(RubyValue::Hash(crate::collections::hash_new(pairs)))
    }
    // `Encoding.locale_charmap` -- the encoding the LOCALE names, which is what
    // `Encoding.default_external` is derived from.
    def self."locale_charmap"(_recv) {
        Ok(RubyValue::Str(crate::string_new(
            encoding::default_external().name().to_string(),
        )))
    }
    // Marshal's hook pair. `_dump` answers the encoding's NAME; `_load`
    // answers what it was handed, which is CRuby's own behavior -- the name
    // round-trips as a String, not back into the singleton.
    def self."_load"(_recv, name) {
        Ok(name.clone())
    }
    def "_dump" cfunc (recv, *_args) {
        Ok(RubyValue::Str(crate::string_new(recv_encoding(recv).name().to_string())))
    }
    def self."default_external"(_recv) {
        Ok(encoding_value(encoding::default_external()))
    }
    def self."default_external="(_recv, arg) {
        let id = arg_encoding(arg)?;
        encoding::set_default_external(id);
        Ok(encoding_value(id))
    }
    def self."default_internal"(_recv) {
        Ok(match encoding::default_internal() {
            Some(id) => encoding_value(id),
            None => RubyValue::Nil,
        })
    }
    def self."default_internal="(_recv, arg) {
        if matches!( *arg, RubyValue::Nil) {
            encoding::set_default_internal(None);
            return Ok(RubyValue::Nil);
        }
        let id = arg_encoding(arg)?;
        encoding::set_default_internal(Some(id));
        Ok(encoding_value(id))
    }

    def "name" | "to_s"(recv) {
        Ok(RubyValue::Str(crate::string_new(recv_encoding(recv).name().to_string())))
    }
    def "inspect"(recv) {
        Ok(RubyValue::Str(crate::string_new(format!("#<Encoding:{}>", recv_encoding(recv).inspect_name()))))
    }
    def "names"(recv) {
        let names = recv_encoding(recv)
            .names()
            .into_iter()
            .map(|n| RubyValue::Str(crate::string_new(n.to_string())))
            .collect();
        Ok(RubyValue::Array(crate::array_new(names)))
    }
    def "ascii_compatible?"(recv) {
        Ok(RubyValue::Bool(recv_encoding(recv).ascii_compatible()))
    }
    def "dummy?"(recv) {
        Ok(RubyValue::Bool(recv_encoding(recv).is_dummy()))
    }
    // An Encoding is a singleton in ruby, so `==`/`eql?`/`hash` are just
    // Kernel's identity ones and ruby declares none of them here. zeo can mint
    // more than one object for the same encoding, so the rows have to compare
    // and hash the encoding INDEX -- but they are marked `inherits`, so
    // `Encoding.instance_methods(false)` and `.owner` still answer Kernel's.
    def "==" | "eql?" inherits (recv, other) {
        let same = matches!(other, RubyValue::Object(o) if o.class_id() == ENCODING_CLASS)
            && recv_encoding(recv) == recv_encoding(other);
        Ok(RubyValue::Bool(same))
    }
    def "hash" inherits (recv) {
        Ok(RubyValue::Int(recv_encoding(recv).0 as i64))
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
            // One side's facts at a time -- never both guards at once, so
            // concurrent `compatible?(a, b)` / `compatible?(b, a)` on two
            // threads can't deadlock on opposite lock orders.
            let (ea, a_empty, a_ascii) = {
                let g = a.lock();
                (g.encoding(), g.is_empty(), g.ascii_only())
            };
            let (eb, b_empty, b_ascii) = {
                let g = b.lock();
                (g.encoding(), g.is_empty(), g.ascii_only())
            };
            if ea == eb {
                return Some(ea);
            }
            // `rb_enc_str_asciionly_p` is ascii-compatible AND 7-bit, so a
            // UTF-16 string is never ascii-only however it reads.
            let a_ascii = a_ascii && ea.ascii_compatible();
            let b_ascii = b_ascii && eb.ascii_compatible();
            // An empty SECOND side always yields, whatever either encoding
            // is. An empty FIRST side keeps its own encoding only when that
            // encoding could have carried the other side's text as it stands.
            if b_empty {
                return Some(ea);
            }
            if a_empty {
                return Some(if ea.ascii_compatible() && b_ascii {
                    ea
                } else {
                    eb
                });
            }
            if !ea.ascii_compatible() || !eb.ascii_compatible() {
                return None;
            }
            // Whichever side is 7-bit yields; the SECOND is asked first, so
            // two ascii-only strings answer the FIRST one's encoding.
            if b_ascii {
                return Some(ea);
            }
            if a_ascii {
                return Some(eb);
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
    // The Unicode release the character data answers from: the single-byte
    // mapping tables were generated from ruby 4.0.6's own converters, and the
    // case mapping comes from Rust's `char`, which tracks the same release.
    const_set(
        owner,
        "UNICODE_VERSION",
        RubyValue::Str(crate::string_new("17.0.0".to_string())),
    );
    for id in encoding::all() {
        // `names()` also answers the runtime selectors (`locale`, `external`,
        // ...), which name no constant of their own in CRuby.
        for name in id.spec_names() {
            for c in const_names(name) {
                const_set(owner, &c, encoding_value(id));
            }
        }
    }
}

/// The Ruby constant spellings of one encoding name. CRuby registers up to
/// two per name, oracle-verified across all 174 of them:
///
/// - the name with every non-alphanumeric character turned into `_` and a
///   leading lowercase letter capitalized (`"Big5-HKSCS:2008"` ->
///   `Big5_HKSCS_2008`, `"eucJP"` -> `EucJP`), but only when the name
///   carries an uppercase letter somewhere -- `"ebcdic-cp-us"` gets no
///   `Ebcdic_cp_us`;
/// - that spelling fully upcased, whenever the name carries a lowercase
///   letter (`WINDOWS_1250` beside `Windows_1250`).
///
/// A name that opens with a digit (`"646"`) spells no constant at all.
fn const_names(name: &str) -> Vec<String> {
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        return Vec::new();
    }
    let mut underscored: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    underscored[..1].make_ascii_uppercase();
    let mut out = Vec::new();
    if name.contains(|c: char| c.is_ascii_uppercase()) {
        out.push(underscored.clone());
    }
    if name.contains(|c: char| c.is_ascii_lowercase()) {
        let upper = underscored.to_ascii_uppercase();
        if !out.contains(&upper) {
            out.push(upper);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::ENCODING_CLASS)
            .unwrap()
            .instance
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }
    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::ENCODING_CLASS)
            .unwrap()
            .class
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }
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
        assert_eq!(
            imethod("name")(&utf8, &[], None)
                .unwrap()
                .to_display_string(),
            "UTF-8"
        );
        assert_eq!(
            imethod("inspect")(&utf8, &[], None)
                .unwrap()
                .to_display_string(),
            "#<Encoding:UTF-8>"
        );
    }

    #[test]
    fn find_resolves_aliases() {
        let found = cmethod("find")(
            &RubyValue::Class(ENCODING_CLASS),
            &[str_val("BINARY")],
            None,
        )
        .unwrap();
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
