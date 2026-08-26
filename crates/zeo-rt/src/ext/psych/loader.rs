//! The load half: yaml-rust2's event stream, read for everything it
//! carries.
//!
//! The events always held the anchor id and the tag; the old sink threw
//! both away, so every `*ref` resolved to `nil` (data loss), every tag was
//! ignored (a wrong type AND a defeated `safe_load` gate), and a `<<` key
//! stayed literal. Nothing needed forking -- only reading.
//!
//! Two rules shape the code:
//!
//!   * A CONTAINER IS REGISTERED EMPTY, at `MappingStart`/`SequenceStart`,
//!     and filled in place. That is what makes a self-referential document
//!     (`--- &1\na: *1`) load as a real cycle instead of recursing.
//!   * The three entry points differ only in what they PERMIT.
//!     `safe_load` permits what the caller passed, `load` permits `Symbol`
//!     and nothing else, `unsafe_load` permits everything -- and aliases
//!     are their own switch, off for both `load` and `safe_load`.
//!
//! `on_event` cannot fail, so the first refusal is parked and re-raised
//! once the parse finishes; nothing is accepted after it.

use super::scanner::{self, Scalar};
use crate::collections::{array_new, hash_new};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal, string_new};
use std::collections::HashMap;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};

/// What a load was asked for.
pub(super) struct LoadOpts {
    /// Class names the document may build. `None` = everything, which is
    /// `unsafe_load`.
    pub permitted: Option<Vec<String>>,
    pub aliases: bool,
    pub symbolize_names: bool,
    pub freeze: bool,
}

impl LoadOpts {
    fn allows(&self, class: &str) -> bool {
        match &self.permitted {
            None => true,
            Some(names) => names.iter().any(|n| n == class),
        }
    }

    fn gate(&self, class: &str) -> Result<(), Signal> {
        if self.allows(class) {
            return Ok(());
        }
        Err(raise_error(
            "Psych::DisallowedClass",
            format!("Tried to load unspecified class: {class}"),
        ))
    }
}

enum Container {
    Seq(RubyValue),
    /// The hash, its pending KEY (and whether that key was a plain `<<`),
    /// and the values a `<<` key collected.
    Map(RubyValue, Option<(RubyValue, bool)>, Vec<RubyValue>),
}

pub(super) struct Loader<'a> {
    docs: Vec<RubyValue>,
    stack: Vec<Container>,
    anchors: HashMap<usize, RubyValue>,
    /// The tag on the container whose Start is being handled, kept until
    /// its End can act on it.
    tags: Vec<Option<String>>,
    opts: &'a LoadOpts,
    failed: Option<Signal>,
}

impl<'a> Loader<'a> {
    pub(super) fn new(opts: &'a LoadOpts) -> Loader<'a> {
        Loader {
            docs: Vec::new(),
            stack: Vec::new(),
            anchors: HashMap::new(),
            tags: Vec::new(),
            opts,
            failed: None,
        }
    }

    /// A finished value: into the open container, or -- with none open --
    /// it IS the document. `anchor` is yaml-rust2's id, `0` for none.
    fn accept(&mut self, v: RubyValue, anchor: usize, merge_key: bool) {
        if self.failed.is_some() {
            return;
        }
        if anchor != 0 {
            self.anchors.insert(anchor, v.clone());
        }
        self.place_marked(v, merge_key);
    }

    fn place(&mut self, v: RubyValue) {
        self.place_marked(v, false);
    }

    /// `merge_key` says this value, if it lands in a KEY position, is the
    /// plain untagged `<<` that means merge -- and not the ordinary string
    /// key `!!str '<<'` spells.
    fn place_marked(&mut self, v: RubyValue, merge_key: bool) {
        match self.stack.last_mut() {
            Some(Container::Seq(a)) => {
                if let RubyValue::Array(arr) = a {
                    arr.lock().push(v);
                }
            }
            Some(Container::Map(h, pending, merges)) => match pending.take() {
                Some((k, is_merge)) => {
                    // A `<<` key collects rather than storing: its values
                    // are spliced at MappingEnd, under the keys written
                    // here, which is the order psych produces.
                    //
                    // ...but only when there is something to merge. `<<: 1`
                    // is an ordinary key with an ordinary value, and psych
                    // keeps it as one.
                    if is_merge && is_mergeable(&v) {
                        merges.push(v);
                        return;
                    }
                    if let RubyValue::Hash(hash) = h {
                        crate::collections::hash_set(hash, k, v);
                    }
                }
                None => *pending = Some((v, merge_key)),
            },
            None => self.docs.push(v),
        }
    }

    fn fail(&mut self, e: Signal) {
        if self.failed.is_none() {
            self.failed = Some(e);
        }
    }

    /// One plain or quoted scalar, with whatever tag it carried.
    fn scalar(&mut self, value: &str, style: TScalarStyle, tag: Option<&str>) -> RubyValue {
        if let Some(tag) = tag {
            match self.tagged_scalar(value, tag) {
                Ok(Some(v)) => return v,
                Ok(None) => {}
                Err(e) => {
                    self.fail(e);
                    return RubyValue::Nil;
                }
            }
        }
        // Only a PLAIN scalar is scanned. A quoted or block one is a
        // String whatever it spells, which is what keeps `'017'` a string.
        if style != TScalarStyle::Plain {
            return RubyValue::Str(string_new(value.to_string()));
        }
        match scanner::resolve(value) {
            Scalar::Plain(v) => v,
            Scalar::Symbol(name) => match self.opts.gate("Symbol") {
                Ok(()) => RubyValue::Symbol(crate::Symbol::intern(&name)),
                Err(e) => {
                    self.fail(e);
                    RubyValue::Nil
                }
            },
            Scalar::Date(y, m, d) => match self.opts.gate("Date").and_then(|()| build_date(y, m, d))
            {
                Ok(v) => v,
                Err(e) => {
                    self.fail(e);
                    RubyValue::Nil
                }
            },
            Scalar::Timestamp(text) => {
                match self.opts.gate("Time").and_then(|()| build_time(&text)) {
                    Ok(v) => v,
                    Err(e) => {
                        self.fail(e);
                        RubyValue::Nil
                    }
                }
            }
        }
    }

    /// A scalar carrying an explicit tag. `Ok(None)` means the tag says
    /// nothing this loader acts on, so the ordinary rules apply.
    fn tagged_scalar(&mut self, value: &str, tag: &str) -> Result<Option<RubyValue>, Signal> {
        let v = match tag {
            "tag:yaml.org,2002:str" => RubyValue::Str(string_new(value.to_string())),
            // `!!int` COERCES what reads as one and leaves the rest
            // alone -- `!!int 'zz'` is the string, not zero.
            "tag:yaml.org,2002:int" => match scanner::resolve(value) {
                Scalar::Plain(v @ (RubyValue::Int(_) | RubyValue::BigInt(_))) => v,
                _ => match value.trim().parse::<num_bigint::BigInt>() {
                    Ok(n) => crate::builtins::integer::int_value(n),
                    Err(_) => return Ok(None),
                },
            },
            // `!!float`, by contrast, INSISTS: ruby hands the text to
            // `Float()` and lets its ArgumentError out.
            "tag:yaml.org,2002:float" => match scanner::resolve(value) {
                Scalar::Plain(RubyValue::Float(f)) => RubyValue::Float(f),
                Scalar::Plain(RubyValue::Int(n)) => RubyValue::Float(n as f64),
                _ => match value.trim().parse::<f64>() {
                    Ok(f) => RubyValue::Float(f),
                    Err(_) => {
                        return Err(raise_error(
                            "ArgumentError",
                            format!("invalid value for Float(): \"{value}\""),
                        ));
                    }
                },
            },
            "tag:yaml.org,2002:bool" => {
                RubyValue::Bool(matches!(value.to_ascii_lowercase().as_str(), "yes" | "true" | "on"))
            }
            // `! ''` -- psych's spelling for "the empty scalar, and I mean
            // it", which is how a nil KEY survives a dump.
            "!" if value.is_empty() => RubyValue::Nil,
            "tag:yaml.org,2002:binary" => {
                let bytes = base64_decode(value);
                RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT))
            }
            // `!ruby/symbol foo` is the tagged spelling of `:foo`, gated
            // exactly like the plain one.
            "!ruby/symbol" | "tag:yaml.org,2002:ruby/symbol" => {
                self.opts.gate("Symbol")?;
                RubyValue::Symbol(crate::Symbol::intern(value))
            }
            other if other.starts_with("!ruby/object:") => {
                let name = other.trim_start_matches("!ruby/object:");
                self.opts.gate(name)?;
                // Permitted, but building an arbitrary object from a
                // scalar is not a shape psych produces.
                RubyValue::Str(string_new(value.to_string()))
            }
            _ => return Ok(None),
        };
        Ok(Some(v))
    }
}

/// Whether a `<<` value is something to splice: a mapping, or a sequence
/// holding one.
fn is_mergeable(v: &RubyValue) -> bool {
    match v {
        RubyValue::Hash(_) => true,
        RubyValue::Array(a) => a.lock().iter().any(|e| matches!(e, RubyValue::Hash(_))),
        _ => false,
    }
}

/// `Date.new(y, m, d, Date::GREGORIAN)` -- a YAML date is PROLEPTIC
/// Gregorian, with no Julian reform behind it, which is why psych's dates
/// carry `-Infj` where a bare `Date.new` carries the Italian reform day.
fn build_date(y: i32, m: u32, d: u32) -> Result<RubyValue, Signal> {
    let cls = class_named("Date")?;
    crate::dispatch::send_value(
        &cls,
        crate::Symbol::intern("new"),
        &[
            RubyValue::Int(i64::from(y)),
            RubyValue::Int(i64::from(m)),
            RubyValue::Int(i64::from(d)),
            RubyValue::Float(f64::NEG_INFINITY),
        ],
        None,
    )
}

/// A timestamp scalar as a real `Time`.
///
/// A timestamp with NO ZONE is UTC, not local -- that is YAML's rule, and
/// psych then hands back the local-time view of it. So `2001-12-14
/// 21:59:43` is 13:59:43 on the American west coast, not 21:59:43 there.
fn build_time(text: &str) -> Result<RubyValue, Signal> {
    let cls = class_named("Time")?;
    let zoned = has_zone(text);
    let text = match zoned {
        true => text.to_string(),
        false => format!("{text} Z"),
    };
    let arg = RubyValue::Str(string_new(text));
    let t = crate::dispatch::send_value(&cls, crate::Symbol::intern("parse"), &[arg], None)?;
    match zoned {
        true => Ok(t),
        false => crate::dispatch::send_value(&t, crate::Symbol::intern("localtime"), &[], None),
    }
}

/// Whether a timestamp names its own zone -- a trailing `Z`, or a `+`/`-`
/// offset AFTER the clock (the date's own hyphens do not count).
fn has_zone(text: &str) -> bool {
    let Some(clock) = text.find([':']) else {
        return false;
    };
    let tail = &text[clock..];
    tail.ends_with('Z') || tail.ends_with('z') || tail.contains('+') || tail[1..].contains('-')
}

/// A class by name, builtin ids included.
///
/// `runtime_class_id_by_name` only knows classes a PROGRAM minted, so a
/// builtin like `Date` never matched and a permitted date scalar raised
/// `NameError` instead of loading.
fn class_named(name: &str) -> Result<RubyValue, Signal> {
    let builtin = match name {
        "Date" => Some(zeo_abi::DATE_CLASS),
        "Time" => Some(zeo_abi::TIME_CLASS),
        "DateTime" => Some(zeo_abi::DATETIME_CLASS),
        "Range" => Some(zeo_abi::RANGE_CLASS),
        _ => None,
    };
    builtin
        .or_else(|| crate::runtime_meta::runtime_class_id_by_name(name))
        .map(RubyValue::Class)
        .ok_or_else(|| {
            raise_error(
                "NameError",
                format!("uninitialized constant {name} (require it to load this document)"),
            )
        })
}

/// YAML's `!!binary` payload. Whitespace is layout, not data.
fn base64_decode(text: &str) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for c in text.bytes() {
        if c == b'=' {
            break;
        }
        let Some(i) = TABLE.iter().position(|&t| t == c) else {
            continue; // whitespace and anything else the encoder wrapped with
        };
        acc = (acc << 6) | i as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

impl MarkedEventReceiver for Loader<'_> {
    fn on_event(&mut self, ev: Event, _mark: Marker) {
        if self.failed.is_some() {
            return;
        }
        match ev {
            Event::Scalar(value, style, anchor, tag) => {
                let tag = tag.map(tag_text);
                let merge_key =
                    value == "<<" && style == TScalarStyle::Plain && tag.is_none();
                let v = self.scalar(&value, style, tag.as_deref());
                self.accept(v, anchor, merge_key);
            }
            Event::SequenceStart(anchor, tag) => {
                // Registered EMPTY, then filled -- see the module doc.
                let v = RubyValue::Array(array_new(Vec::new()));
                if anchor != 0 {
                    self.anchors.insert(anchor, v.clone());
                }
                self.tags.push(tag.map(tag_text));
                self.stack.push(Container::Seq(v));
            }
            Event::MappingStart(anchor, tag) => {
                let v = RubyValue::Hash(hash_new(Vec::new()));
                if anchor != 0 {
                    self.anchors.insert(anchor, v.clone());
                }
                let tag = tag.map(tag_text);
                if let Some(t) = &tag
                    && let Err(e) = self.gate_container_tag(t)
                {
                    self.fail(e);
                    return;
                }
                self.tags.push(tag);
                self.stack.push(Container::Map(v, None, Vec::new()));
            }
            Event::SequenceEnd => {
                let tag = self.tags.pop().flatten();
                let Some(Container::Seq(v)) = self.stack.pop() else {
                    return;
                };
                // `!!omap` is written as a sequence of one-pair mappings
                // and loads as ONE Hash, in that order.
                let v = match tag.as_deref() {
                    Some("tag:yaml.org,2002:omap") => flatten_omap(&v),
                    _ => v,
                };
                self.place(v);
            }
            Event::MappingEnd => {
                let tag = self.tags.pop().flatten();
                let Some(Container::Map(v, _, merges)) = self.stack.pop() else {
                    return;
                };
                self.splice_merges(&v, merges);
                // `!ruby/range` is written as a `begin`/`end`/`excl`
                // mapping and loads back as the Range it came from.
                let v = match tag.as_deref() {
                    Some("!ruby/range") => match self.rebuild_range(&v) {
                        Ok(r) => r,
                        Err(e) => {
                            self.fail(e);
                            return;
                        }
                    },
                    _ => v,
                };
                self.place(v);
            }
            Event::Alias(id) => {
                if !self.opts.aliases {
                    self.fail(raise_error(
                        "Psych::AliasesNotEnabled",
                        "Alias parsing was not enabled. To enable it, pass `aliases: true` to \
                         `Psych::load` or `Psych::safe_load`."
                            .to_string(),
                    ));
                    return;
                }
                match self.anchors.get(&id).cloned() {
                    Some(v) => self.place(v),
                    None => self.fail(raise_error(
                        "Psych::AnchorNotDefined",
                        format!("An alias referenced an unknown anchor: {id}"),
                    )),
                }
            }
            _ => {}
        }
    }
}

impl Loader<'_> {
    /// `{begin:, end:, excl:}` back into the Range it was dumped from.
    fn rebuild_range(&self, v: &RubyValue) -> Result<RubyValue, Signal> {
        self.opts.gate("Range")?;
        let RubyValue::Hash(h) = v else {
            return Ok(v.clone());
        };
        let field = |name: &str| {
            crate::collections::hash_get(h, &RubyValue::Str(string_new(name.to_string())))
        };
        let cls = class_named("Range")?;
        crate::dispatch::send_value(
            &cls,
            crate::Symbol::intern("new"),
            &[field("begin"), field("end"), field("excl")],
            None,
        )
    }

    /// A `!!set` is `Psych::Set`, which the caller has to permit; every
    /// other container tag psych models loads as the plain container.
    fn gate_container_tag(&self, tag: &str) -> Result<(), Signal> {
        match tag {
            "tag:yaml.org,2002:set" => self.opts.gate("Psych::Set"),
            "!ruby/range" => self.opts.gate("Range"),
            t if t.starts_with("!ruby/object:") => {
                self.opts.gate(t.trim_start_matches("!ruby/object:"))
            }
            _ => Ok(()),
        }
    }

    /// Splice what a `<<` key collected. Psych merges BACKWARDS through a
    /// list and never overwrites a key the mapping wrote itself, which is
    /// why the host's own pairs go in last.
    fn splice_merges(&mut self, host: &RubyValue, merges: Vec<RubyValue>) {
        if merges.is_empty() {
            return;
        }
        let RubyValue::Hash(target) = host else {
            return;
        };
        let own = crate::collections::hash_pairs(target);
        target.lock().clear();
        for source in merges {
            let sources: Vec<RubyValue> = match &source {
                RubyValue::Array(a) => a.lock().iter().cloned().collect(),
                other => vec![other.clone()],
            };
            for s in sources.iter().rev() {
                if let RubyValue::Hash(h) = s {
                    for (k, v) in crate::collections::hash_pairs(h) {
                        crate::collections::hash_set(target, k, v);
                    }
                }
            }
        }
        for (k, v) in own {
            crate::collections::hash_set(target, k, v);
        }
    }

    pub(super) fn finish(self) -> Result<Vec<RubyValue>, Signal> {
        match self.failed {
            Some(e) => Err(e),
            None => {
                if self.opts.freeze {
                    for d in &self.docs {
                        freeze_tree(d);
                    }
                }
                if self.opts.symbolize_names {
                    return Ok(self.docs.iter().map(symbolize).collect());
                }
                Ok(self.docs)
            }
        }
    }
}

/// yaml-rust2 hands a tag as `(handle, suffix)`; psych compares the whole
/// name, so put it back together.
fn tag_text(t: Tag) -> String {
    match t.handle.as_str() {
        "" => t.suffix,
        h => format!("{h}{}", t.suffix),
    }
}

/// `[{a: 1}, {b: 2}]` -> `{a: 1, b: 2}` -- what `!!omap` means.
fn flatten_omap(v: &RubyValue) -> RubyValue {
    let RubyValue::Array(a) = v else {
        return v.clone();
    };
    let mut pairs = Vec::new();
    for item in a.lock().iter() {
        if let RubyValue::Hash(h) = item {
            pairs.extend(crate::collections::hash_pairs(h));
        }
    }
    RubyValue::Hash(hash_new(pairs))
}

fn symbolize(v: &RubyValue) -> RubyValue {
    match v {
        RubyValue::Hash(h) => {
            let pairs = crate::collections::hash_pairs(h)
                .into_iter()
                .map(|(k, val)| {
                    let k = match &k {
                        RubyValue::Str(s) => {
                            RubyValue::Symbol(crate::Symbol::intern(&s.lock().to_utf8_lossy()))
                        }
                        other => other.clone(),
                    };
                    (k, symbolize(&val))
                })
                .collect();
            RubyValue::Hash(hash_new(pairs))
        }
        RubyValue::Array(a) => {
            RubyValue::Array(array_new(a.lock().iter().map(symbolize).collect()))
        }
        other => other.clone(),
    }
}

fn freeze_tree(v: &RubyValue) {
    match v {
        RubyValue::Str(s) => s.set_frozen(),
        RubyValue::Array(a) => {
            let items = a.lock().clone();
            for e in &items {
                freeze_tree(e);
            }
            a.set_frozen();
        }
        RubyValue::Hash(h) => {
            for (k, val) in crate::collections::hash_pairs(h) {
                freeze_tree(&k);
                freeze_tree(&val);
            }
            h.set_frozen();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_ignores_the_layout_the_encoder_added() {
        assert_eq!(base64_decode("aGk="), b"hi");
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ="), b"hello world");
        assert_eq!(base64_decode("aG\nVs\tbG8gd29ybGQ="), b"hello world");
        assert_eq!(base64_decode(""), b"");
        // Nothing it is handed can panic it.
        for b in 0u8..=255 {
            let _ = base64_decode(&String::from_utf8_lossy(&[b, b'=', b]));
        }
    }

    #[test]
    fn a_tag_is_put_back_together_from_its_halves() {
        assert_eq!(
            tag_text(Tag {
                handle: "tag:yaml.org,2002:".to_string(),
                suffix: "str".to_string()
            }),
            "tag:yaml.org,2002:str"
        );
        assert_eq!(
            tag_text(Tag {
                handle: String::new(),
                suffix: "!ruby/symbol".to_string()
            }),
            "!ruby/symbol"
        );
    }
}
