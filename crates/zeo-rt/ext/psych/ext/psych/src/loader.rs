//! The load half: a parsed [`Node`] tree walked into Ruby values.
//!
//! This reads [`nodes`](super::nodes) rather than yaml-rust2's events
//! directly, for the reason that module explains: `Psych.parse` needs the
//! tree, and one walk over one tree beats two walks that have to agree.
//!
//! Three rules shape the code:
//!
//!   * A CONTAINER IS REGISTERED EMPTY, before its children are walked, and
//!     filled in place. That is what makes a self-referential document
//!     (`--- &1\na: *1`) load as a real cycle instead of recursing forever.
//!   * The three entry points differ only in what they PERMIT. `safe_load`
//!     permits what the caller passed, `load` permits `Symbol` and nothing
//!     else, `unsafe_load` permits everything -- and aliases are their own
//!     switch, off for both `load` and `safe_load`.
//!   * A TAG IS A REQUEST, NOT A PROMISE. Every `!ruby/...` tag goes through
//!     [`LoadOpts::gate`] before anything is built, so `safe_load` cannot be
//!     talked into constructing a class the caller did not name.

use super::nodes::{Document, Node};
use super::scanner::{self, Scalar};
use crate::collections::{array_new, hash_new};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal, string_new};
use std::collections::HashMap;

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

/// The walk from a parsed tree to Ruby values.
pub(super) struct Revive<'a> {
    opts: &'a LoadOpts,
    /// By anchor NAME. A document that redefines a name keeps the latest,
    /// which is what an alias after the second definition refers to.
    anchors: HashMap<String, RubyValue>,
}

impl<'a> Revive<'a> {
    pub(super) fn new(opts: &'a LoadOpts) -> Revive<'a> {
        Revive {
            opts,
            anchors: HashMap::new(),
        }
    }

    /// Every document, in order. Anchors do not cross a document boundary --
    /// yaml's own rule, and the parser's.
    pub(super) fn documents(&mut self, docs: &[Document]) -> Result<Vec<RubyValue>, Signal> {
        let mut out = Vec::new();
        for doc in docs {
            self.anchors.clear();
            let Some(root) = &doc.root else { continue };
            let mut v = self.node(root)?;
            if self.opts.freeze {
                freeze_tree(&v);
            }
            if self.opts.symbolize_names {
                v = symbolize(&v);
            }
            out.push(v);
        }
        Ok(out)
    }

    fn remember(&mut self, node: &Node, v: &RubyValue) {
        if let Some(name) = node.anchor()
            && !name.is_empty()
        {
            self.anchors.insert(name.to_string(), v.clone());
        }
    }

    pub(super) fn node(&mut self, node: &Node) -> Result<RubyValue, Signal> {
        match node {
            Node::Scalar { value, quoted, tag, .. } => {
                let v = self.scalar(value, *quoted, tag.as_deref())?;
                self.remember(node, &v);
                Ok(v)
            }
            Node::Sequence { children, tag, .. } => self.sequence(node, children, tag.as_deref()),
            Node::Mapping { children, tag, .. } => self.mapping(node, children, tag.as_deref()),
            Node::Alias { anchor } => {
                if !self.opts.aliases {
                    return Err(raise_error(
                        "Psych::AliasesNotEnabled",
                        "Alias parsing was not enabled. To enable it, pass `aliases: true` to \
                         `Psych::load` or `Psych::safe_load`."
                            .to_string(),
                    ));
                }
                self.anchors.get(anchor).cloned().ok_or_else(|| {
                    raise_error(
                        "Psych::AnchorNotDefined",
                        format!("An alias referenced an unknown anchor: {anchor}"),
                    )
                })
            }
        }
    }

    // ---- sequences ---------------------------------------------------------

    fn sequence(
        &mut self,
        node: &Node,
        children: &[Node],
        tag: Option<&str>,
    ) -> Result<RubyValue, Signal> {
        // A SEQUENCE gates almost nothing. Psych's sequence visitor reaches
        // the class loader for `!ruby/array:X` and `!seq:X` and for nothing
        // else: an `!!set`, an `!!omap`, a `!ruby/range` or even a
        // `!ruby/object:X` written as a sequence falls through to a plain
        // Array without ever asking. Measured against ruby 4.0.6 -- gating
        // them here refused four documents `safe_load` accepts.
        if let Some(name) = tag
            .and_then(|t| t.strip_prefix("!ruby/array:").or_else(|| t.strip_prefix("!seq:")))
        {
            self.opts.gate(name)?;
        }
        let arr = array_new(Vec::new());
        let v = RubyValue::Array(arr.clone());
        self.remember(node, &v);
        for child in children {
            let item = self.node(child)?;
            arr.lock().push(item);
        }
        match tag {
            // `!!omap` is written as a sequence of one-pair mappings and
            // loads as ONE ordered map, in that order.
            Some("!omap" | "tag:yaml.org,2002:omap") => flatten_omap(&v),
            // `!ruby/array:X` is an Array SUBCLASS, written as a plain
            // sequence. The elements are already right; only the class is
            // not, and a subclass with no `initialize` of its own takes them
            // through `replace`.
            Some(t) if t.starts_with("!ruby/array:") => {
                subclass_container(t.trim_start_matches("!ruby/array:"), &v)
            }
            _ => Ok(v),
        }
    }

    // ---- mappings ----------------------------------------------------------

    fn mapping(
        &mut self,
        node: &Node,
        children: &[Node],
        tag: Option<&str>,
    ) -> Result<RubyValue, Signal> {
        if let Some(t) = tag {
            self.gate_mapping_tag(t)?;
        }
        let hash = hash_new(Vec::new());
        let v = RubyValue::Hash(hash.clone());
        self.remember(node, &v);

        // A `<<` key COLLECTS rather than storing: its values are spliced
        // once the mapping is complete, under the keys written here, which is
        // the order psych produces.
        let mut merges = Vec::new();
        for pair in children.chunks(2) {
            // An odd child count is a mapping the parser could not finish.
            // Dropping the widow matches what psych's own tree does with it.
            let (Some(kn), Some(vn)) = (pair.first(), pair.get(1)) else {
                break;
            };
            let is_merge = is_merge_key(kn);
            let k = self.node(kn)?;
            let val = self.node(vn)?;
            // `<<: 1` is an ordinary key with an ordinary value, and psych
            // keeps it as one -- only a mergeable value merges.
            if is_merge && is_mergeable(&val) {
                merges.push(val);
                continue;
            }
            crate::collections::hash_set(&hash, k, val);
        }
        splice_merges(&hash, merges);

        match tag {
            Some("!ruby/range") => self.rebuild_range(&v),
            Some("!set" | "tag:yaml.org,2002:set") => subclass_container("Psych::Set", &v),
            Some("!omap" | "tag:yaml.org,2002:omap") => {
                self.opts.gate("Psych::Omap")?;
                subclass_container("Psych::Omap", &v)
            }
            Some(t) if t.starts_with("!ruby/hash-with-ivars:") => {
                self.hash_with_ivars(t.trim_start_matches("!ruby/hash-with-ivars:"), &v)
            }
            Some(t) if t.starts_with("!ruby/hash:") => {
                subclass_container(t.trim_start_matches("!ruby/hash:"), &v)
            }
            Some(t) if t.starts_with("!ruby/struct:") => {
                self.rebuild_struct(t.trim_start_matches("!ruby/struct:"), &v)
            }
            Some(t) if t.starts_with("!ruby/exception:") => {
                self.rebuild_exception(t.trim_start_matches("!ruby/exception:"), &v, t)
            }
            Some(t) if t.starts_with("!ruby/object:") => {
                let name = t.trim_start_matches("!ruby/object:");
                let built = self.rebuild_object(name, &v, t)?;
                // The anchor was registered against the empty Hash, and a
                // self-reference inside the object has to reach the OBJECT.
                // Rewriting it is the best that can be done without a
                // two-phase build; a cycle THROUGH a revived object still
                // resolves to the hash, which is recorded in
                // tests/gaps/a_yaml_cycle_through_a_revived_object.rb.
                self.remember(node, &built);
                Ok(built)
            }
            _ => Ok(v),
        }
    }

    // ---- scalars -----------------------------------------------------------

    /// One plain or quoted scalar, with whatever tag it carried.
    fn scalar(&mut self, value: &str, quoted: bool, tag: Option<&str>) -> Result<RubyValue, Signal> {
        if let Some(tag) = tag
            && let Some(v) = self.tagged_scalar(value, tag)?
        {
            return Ok(v);
        }
        // Only an UNQUOTED scalar is scanned. A quoted or block one is a
        // String whatever it spells, which is what keeps `'017'` a string.
        if quoted {
            return Ok(RubyValue::Str(string_new(value.to_string())));
        }
        match scanner::resolve(value) {
            Scalar::Plain(v) => Ok(v),
            Scalar::Symbol(name) => {
                self.opts.gate("Symbol")?;
                Ok(RubyValue::Symbol(crate::Symbol::intern(&name)))
            }
            Scalar::Date(y, m, d) => {
                self.opts.gate("Date")?;
                build_date(y, m, d)
            }
            Scalar::Timestamp(text) => {
                self.opts.gate("Time")?;
                build_time(&text)
            }
        }
    }

    /// A scalar carrying an explicit tag. `Ok(None)` means the tag says
    /// nothing this loader acts on, so the ordinary rules apply.
    fn tagged_scalar(&mut self, value: &str, tag: &str) -> Result<Option<RubyValue>, Signal> {
        let v = match tag {
            "tag:yaml.org,2002:str" | "!str" => RubyValue::Str(string_new(value.to_string())),
            // `!!int` COERCES what reads as one and leaves the rest alone --
            // `!!int 'zz'` is the string, not zero.
            "tag:yaml.org,2002:int" | "!int" => match scanner::resolve(value) {
                Scalar::Plain(v @ (RubyValue::Int(_) | RubyValue::BigInt(_))) => v,
                _ => match value.trim().parse::<num_bigint::BigInt>() {
                    Ok(n) => crate::builtins::integer::int_value(n),
                    Err(_) => return Ok(None),
                },
            },
            // `!!float`, by contrast, INSISTS: ruby hands the text to
            // `Float()` and lets its ArgumentError out.
            "tag:yaml.org,2002:float" | "!float" => match scanner::resolve(value) {
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
            "tag:yaml.org,2002:bool" | "!bool" => RubyValue::Bool(matches!(
                value.to_ascii_lowercase().as_str(),
                "yes" | "true" | "on"
            )),
            "tag:yaml.org,2002:null" | "!null" => RubyValue::Nil,
            // `! ''` -- psych's spelling for "the empty scalar, and I mean
            // it", which is how a nil KEY survives a dump.
            "!" if value.is_empty() => RubyValue::Nil,
            "tag:yaml.org,2002:binary" | "!binary" => {
                let bytes = base64_decode(value);
                RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT))
            }
            // `!ruby/symbol foo` is the tagged spelling of `:foo`, gated
            // exactly like the plain one.
            "!ruby/symbol" | "tag:yaml.org,2002:ruby/symbol" => {
                self.opts.gate("Symbol")?;
                RubyValue::Symbol(crate::Symbol::intern(value))
            }
            // A class or a module by name. Psych looks it up rather than
            // defining it, so a name nothing defines is an error and not a
            // fresh empty class.
            "!ruby/class" | "!ruby/module" => {
                self.opts.gate(value)?;
                class_named(value)?
            }
            "!ruby/regexp" => {
                self.opts.gate("Regexp")?;
                build_regexp(value)?
            }
            // A String SUBCLASS, written as a plain scalar.
            other if other.starts_with("!ruby/string:") => {
                let name = other.trim_start_matches("!ruby/string:");
                self.opts.gate(name)?;
                let text = RubyValue::Str(string_new(value.to_string()));
                crate::dispatch::send_value(
                    &class_named(name)?,
                    crate::Symbol::intern("new"),
                    &[text],
                    None,
                )?
            }
            other if other.starts_with("!ruby/object:") => {
                let name = other.trim_start_matches("!ruby/object:");
                self.opts.gate(name)?;
                // Permitted, but building an arbitrary object from a scalar
                // is not a shape psych produces.
                RubyValue::Str(string_new(value.to_string()))
            }
            _ => return Ok(None),
        };
        Ok(Some(v))
    }

    // ---- reviving a tagged mapping ----------------------------------------

    /// `{begin:, end:, excl:}` back into the Range it was dumped from.
    fn rebuild_range(&self, v: &RubyValue) -> Result<RubyValue, Signal> {
        self.opts.gate("Range")?;
        let RubyValue::Hash(h) = v else {
            return Ok(v.clone());
        };
        let field = |name: &str| {
            crate::collections::hash_get(h, &RubyValue::Str(string_new(name.to_string())))
        };
        crate::dispatch::send_value(
            &class_named("Range")?,
            crate::Symbol::intern("new"),
            &[field("begin"), field("end"), field("excl")],
            None,
        )
    }

    /// `!ruby/object:Widget` -- allocate the class and fill it.
    ///
    /// Psych's own order, and each step matters: `init_with` first so a class
    /// that knows how to rebuild itself does; `yaml_initialize` next, which
    /// is what `Gem::Specification` defines and the reason a `.gem` could not
    /// be read before this; and plain ivar assignment last, which is what
    /// most objects want.
    ///
    /// `allocate` rather than `new`, because `new` runs `initialize` with no
    /// arguments and most classes refuse that. The object is therefore
    /// UNINITIALIZED until the fill, which is exactly psych's contract.
    fn rebuild_object(
        &mut self,
        name: &str,
        v: &RubyValue,
        tag: &str,
    ) -> Result<RubyValue, Signal> {
        self.opts.gate(name)?;
        // Psych's two numeric special cases, written as mappings.
        match name {
            "Complex" | "Rational" => return self.rebuild_number(name, v),
            _ => {}
        }
        let RubyValue::Hash(h) = v else {
            return Ok(v.clone());
        };
        // A name nothing defines is an error, and the message is ruby's own.
        // Measured: `unsafe_load("--- !ruby/object:NoSuchClass\\nx: 1\\n")`
        // raises `ArgumentError: undefined class/module NoSuchClass` rather
        // than answering an Object carrying the ivars.
        let cls = class_named(name).map_err(|_| {
            raise_error(
                "ArgumentError",
                format!("undefined class/module {name}"),
            )
        })?;
        let obj = crate::dispatch::send_value(&cls, crate::Symbol::intern("allocate"), &[], None)?;
        self.init_with(&obj, h, tag)
    }

    /// The fill half of a revival, shared by object, struct and exception.
    fn init_with(
        &mut self,
        obj: &RubyValue,
        h: &crate::collections::RHash,
        tag: &str,
    ) -> Result<RubyValue, Signal> {
        let map = RubyValue::Hash(h.clone());
        if responds_to(obj, "init_with") {
            let coder = self.coder(tag, &map)?;
            crate::dispatch::send_value(obj, crate::Symbol::intern("init_with"), &[coder], None)?;
            return Ok(obj.clone());
        }
        // There is NO `yaml_initialize` rung. psych had one and 5.4 removed
        // it, so a class defining only `yaml_initialize` has its ivars
        // assigned and its method never called -- verified against ruby
        // 4.0.6, which answers `#<KnowsYamlInitialize tag=nil seen=nil>`.
        // `Gem::Specification` is unaffected because it bridges the two
        // itself: its `init_with(coder)` calls its own
        // `yaml_initialize(coder.tag, coder.map)`.
        for (k, val) in crate::collections::hash_pairs(h) {
            let name = format!("@{}", ivar_name(&k));
            crate::dispatch::send_value(
                obj,
                crate::Symbol::intern("instance_variable_set"),
                &[RubyValue::Str(string_new(name)), val],
                None,
            )?;
        }
        Ok(obj.clone())
    }

    /// `Psych::Coder.new(tag)` with its map filled -- what `init_with`
    /// receives.
    fn coder(&self, tag: &str, map: &RubyValue) -> Result<RubyValue, Signal> {
        let cls = class_named("Psych::Coder")?;
        let coder = crate::dispatch::send_value(
            &cls,
            crate::Symbol::intern("new"),
            &[RubyValue::Str(string_new(tag.to_string()))],
            None,
        )?;
        crate::dispatch::send_value(
            &coder,
            crate::Symbol::intern("map="),
            std::slice::from_ref(map),
            None,
        )?;
        Ok(coder)
    }

    /// `!ruby/object:Complex` / `:Rational`, which psych writes as a mapping
    /// of parts rather than as a string.
    fn rebuild_number(&self, name: &str, v: &RubyValue) -> Result<RubyValue, Signal> {
        let RubyValue::Hash(h) = v else {
            return Ok(v.clone());
        };
        let field = |k: &str| {
            crate::collections::hash_get(h, &RubyValue::Str(string_new(k.to_string())))
        };
        let (a, b) = match name {
            "Complex" => (field("real"), field("image")),
            _ => (field("numerator"), field("denominator")),
        };
        let main = crate::dispatch::main_object();
        crate::dispatch::send_value(&main, crate::Symbol::intern(name), &[a, b], None)
    }

    /// `!ruby/struct:Point` -- a Struct subclass, whose members are the keys.
    fn rebuild_struct(&mut self, name: &str, v: &RubyValue) -> Result<RubyValue, Signal> {
        let gated = if name.is_empty() { "Struct" } else { name };
        self.opts.gate(gated)?;
        // Psych turns each member name into a Symbol to compare it, and that
        // goes through the same class loader the gate lives in -- so
        // `safe_load(struct, permitted_classes: ["Point"])` still refuses
        // until `Symbol` is named too. Surprising, measured, and ruby's.
        self.opts.gate("Symbol")?;
        let RubyValue::Hash(h) = v else {
            return Ok(v.clone());
        };
        let cls = class_named(gated)?;
        // A Struct takes its members positionally, in the order the class
        // declares them -- NOT in the order the document wrote them, which a
        // hand-edited file may disagree about.
        let members =
            crate::dispatch::send_value(&cls, crate::Symbol::intern("members"), &[], None)?;
        let RubyValue::Array(names) = &members else {
            return Ok(v.clone());
        };
        let ordered: Vec<RubyValue> = names
            .lock()
            .iter()
            .map(|m| {
                let key = RubyValue::Str(string_new(m.to_display_string()));
                crate::collections::hash_get(h, &key)
            })
            .collect();
        crate::dispatch::send_value(&cls, crate::Symbol::intern("new"), &ordered, None)
    }

    /// `!ruby/exception:RuntimeError` -- built through `exception`, so the
    /// message lands where `#message` reads it, then filled with whatever
    /// else the document carried.
    fn rebuild_exception(
        &mut self,
        name: &str,
        v: &RubyValue,
        tag: &str,
    ) -> Result<RubyValue, Signal> {
        self.opts.gate(name)?;
        let RubyValue::Hash(h) = v else {
            return Ok(v.clone());
        };
        let cls = class_named(name)?;
        let message =
            crate::collections::hash_get(h, &RubyValue::Str(string_new("message".to_string())));
        let exc = crate::dispatch::send_value(
            &cls,
            crate::Symbol::intern("exception"),
            std::slice::from_ref(&message),
            None,
        )?;
        // `message` is already on the exception; the rest are ivars.
        let rest: Vec<(RubyValue, RubyValue)> = crate::collections::hash_pairs(h)
            .into_iter()
            .filter(|(k, _)| ivar_name(k) != "message")
            .collect();
        self.init_with(&exc, &hash_new(rest), tag)
    }

    /// `!ruby/hash-with-ivars:X` -- a Hash subclass written as
    /// `{elements: {...}, ivars: {...}}`.
    fn hash_with_ivars(&mut self, name: &str, v: &RubyValue) -> Result<RubyValue, Signal> {
        self.opts.gate(name)?;
        let RubyValue::Hash(h) = v else {
            return Ok(v.clone());
        };
        let at = |k: &str| {
            crate::collections::hash_get(h, &RubyValue::Str(string_new(k.to_string())))
        };
        let built = subclass_container(name, &at("elements"))?;
        if let RubyValue::Hash(ivars) = at("ivars") {
            for (k, val) in crate::collections::hash_pairs(&ivars) {
                let ivar = format!("@{}", ivar_name(&k));
                crate::dispatch::send_value(
                    &built,
                    crate::Symbol::intern("instance_variable_set"),
                    &[RubyValue::Str(string_new(ivar)), val],
                    None,
                )?;
            }
        }
        Ok(built)
    }

    /// Every MAPPING tag goes through the gate BEFORE its children are
    /// walked, so `safe_load` refuses at the tag rather than after building
    /// what the tag asked for.
    ///
    /// `!!omap` is the one that is NOT here: psych gates it on a MAPPING and
    /// not on a SEQUENCE, because its two visitors reach `Psych::Omap` by
    /// different routes -- one through the class loader the gate lives in and
    /// one by construction. Measured against ruby 4.0.6, not reasoned:
    /// `safe_load("--- !!omap\na: 1\n")` refuses and
    /// `safe_load("--- !!omap\n- a: 1\n")` loads. So the mapping arm gates it
    /// itself and the sequence arm does not.
    fn gate_mapping_tag(&self, tag: &str) -> Result<(), Signal> {
        match tag {
            "!set" | "tag:yaml.org,2002:set" => self.opts.gate("Psych::Set"),
            "!ruby/range" => self.opts.gate("Range"),
            t => {
                for prefix in [
                    "!ruby/object:",
                    "!ruby/struct:",
                    "!ruby/exception:",
                    "!ruby/hash-with-ivars:",
                    "!ruby/hash:",
                    "!ruby/array:",
                ] {
                    if let Some(name) = t.strip_prefix(prefix) {
                        let named = if name.is_empty() { "Struct" } else { name };
                        return self.opts.gate(named);
                    }
                }
                Ok(())
            }
        }
    }
}

/// Whether a mapping KEY is the plain untagged `<<` that means merge -- and
/// not the ordinary string key `!!str '<<'` spells.
fn is_merge_key(node: &Node) -> bool {
    matches!(
        node,
        Node::Scalar { value, quoted: false, tag: None, .. } if value == "<<"
    )
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

/// Splice what a `<<` key collected. Psych merges BACKWARDS through a list
/// and never overwrites a key the mapping wrote itself, which is why the
/// host's own pairs go in last.
fn splice_merges(target: &crate::collections::RHash, merges: Vec<RubyValue>) {
    if merges.is_empty() {
        return;
    }
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

/// The bare name behind a mapping key, for an ivar assignment. Psych writes
/// ivar names WITHOUT the `@`, so `{"name" => x}` becomes `@name`.
fn ivar_name(k: &RubyValue) -> String {
    k.to_display_string()
}

fn responds_to(v: &RubyValue, name: &str) -> bool {
    crate::dispatch::responds_to_value(v, crate::Symbol::intern(name), true)
}

/// A container of `name`'s class holding what `source` holds.
///
/// `new` then `replace`, rather than `allocate`: a Hash or Array subclass
/// with an `initialize` of its own still gets it run, and `replace` is what
/// psych's own `Psych::Set`/`Omap` path amounts to.
fn subclass_container(name: &str, source: &RubyValue) -> Result<RubyValue, Signal> {
    let cls = class_named(name)?;
    let built = crate::dispatch::send_value(&cls, crate::Symbol::intern("new"), &[], None)?;
    crate::dispatch::send_value(
        &built,
        crate::Symbol::intern("replace"),
        std::slice::from_ref(source),
        None,
    )?;
    Ok(built)
}

/// `!!omap` written as a sequence: `[{a: 1}, {b: 2}]` -> an ordered map.
fn flatten_omap(v: &RubyValue) -> Result<RubyValue, Signal> {
    let RubyValue::Array(a) = v else {
        return Ok(v.clone());
    };
    let mut pairs = Vec::new();
    for item in a.lock().iter() {
        if let RubyValue::Hash(h) = item {
            pairs.extend(crate::collections::hash_pairs(h));
        }
    }
    subclass_container("Psych::Omap", &RubyValue::Hash(hash_new(pairs)))
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
/// psych then hands back the local-time view of it. So `2001-12-14 21:59:43`
/// is 13:59:43 on the American west coast, not 21:59:43 there.
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

/// `!ruby/regexp /body/flags` back into the Regexp it was dumped from.
fn build_regexp(text: &str) -> Result<RubyValue, Signal> {
    // The dumped form is `/source/flags`, and the source may hold slashes of
    // its own -- so the closing one is the LAST, not the second. A slash
    // INSIDE the source is written `\/` and comes back unescaped, which is
    // psych's own `gsub('\/', '/')` and the difference between a pattern that
    // matches `a/b` and one that matches nothing.
    let body = text.strip_prefix('/').unwrap_or(text);
    let (source, flags) = match body.rfind('/') {
        Some(at) => (&body[..at], &body[at + 1..]),
        None => (body, ""),
    };
    let source = source.replace("\\/", "/");
    let mut options = 0i64;
    for f in flags.chars() {
        options |= match f {
            'i' => 1,
            'x' => 2,
            'm' => 4,
            _ => 0,
        };
    }
    let cls = class_named("Regexp")?;
    crate::dispatch::send_value(
        &cls,
        crate::Symbol::intern("new"),
        &[
            RubyValue::Str(string_new(source)),
            RubyValue::Int(options),
        ],
        None,
    )
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

/// A class by name, builtin ids and nested names included.
///
/// `runtime_class_id_by_name` only knows classes a PROGRAM minted, so a
/// builtin like `Date` never matched and a permitted date scalar raised
/// `NameError` instead of loading. A nested name (`Gem::Specification`) is
/// walked one `::` at a time, because the runtime table is keyed by the whole
/// name only for classes a program declared at top level.
pub(super) fn class_named(name: &str) -> Result<RubyValue, Signal> {
    let builtin = match name {
        "Date" => Some(zeo_abi::DATE_CLASS),
        "Time" => Some(zeo_abi::TIME_CLASS),
        "DateTime" => Some(zeo_abi::DATETIME_CLASS),
        "Range" => Some(zeo_abi::RANGE_CLASS),
        "Regexp" => Some(zeo_abi::REGEXP_CLASS),
        "Object" => Some(zeo_abi::OBJECT_CLASS),
        _ => None,
    };
    if let Some(id) = builtin {
        return Ok(RubyValue::Class(id));
    }
    if let Some(id) = crate::runtime_meta::runtime_class_id_by_name(name) {
        return Ok(RubyValue::Class(id));
    }
    // Through `Module#const_get` rather than the constant table, because
    // reading a constant is what RUNS ITS AUTOLOAD -- and the table read does
    // not. rubygems declares `Gem::Dependency` and every one of its
    // neighbours as an autoload, so a table read answered "no such class" for
    // a class that was one line away from existing, and a `.gem` could not be
    // opened. `const_get` also walks `A::B::C` itself, which is the rule
    // ruby's own `resolve_class` follows.
    let object = RubyValue::Class(zeo_abi::OBJECT_CLASS);
    let arg = RubyValue::Str(string_new(name.to_string()));
    crate::dispatch::send_value(&object, crate::Symbol::intern("const_get"), &[arg], None).map_err(
        |_| {
            raise_error(
                "NameError",
                format!("uninitialized constant {name} (require it to load this document)"),
            )
        },
    )
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
        RubyValue::Array(a) => RubyValue::Array(array_new(a.lock().iter().map(symbolize).collect())),
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
    fn a_merge_key_is_only_the_plain_untagged_one() {
        let plain = Node::Scalar {
            value: "<<".to_string(),
            style: super::super::nodes::style::PLAIN,
            quoted: false,
            tag: None,
            anchor: None,
        };
        assert!(is_merge_key(&plain));

        let quoted = Node::Scalar {
            value: "<<".to_string(),
            style: super::super::nodes::style::SINGLE_QUOTED,
            quoted: true,
            tag: None,
            anchor: None,
        };
        assert!(!is_merge_key(&quoted), "a quoted << is an ordinary key");

        let tagged = Node::Scalar {
            value: "<<".to_string(),
            style: super::super::nodes::style::PLAIN,
            quoted: false,
            tag: Some("tag:yaml.org,2002:str".to_string()),
            anchor: None,
        };
        assert!(!is_merge_key(&tagged), "an explicitly tagged << is a key");
    }

    /// The closing slash is the LAST one, so a pattern holding slashes of its
    /// own survives the round trip.
    #[test]
    fn a_regexp_body_may_hold_slashes() {
        let text = "/a\\/b/im";
        let body = text.strip_prefix('/').unwrap();
        let at = body.rfind('/').unwrap();
        assert_eq!(&body[..at], "a\\/b");
        assert_eq!(&body[at + 1..], "im");
    }
}
