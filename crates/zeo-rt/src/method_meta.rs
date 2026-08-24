//! The compile-time-baked facts about each user-defined method that Ruby can
//! ask back for: its parameter list (`Method#arity`/`#parameters`), where it
//! was written (`#source_location`, and the ` file:line` tail of `#inspect`),
//! and -- for an alias -- the name it was born under (`#original_name`).
//!
//! The dispatch tables themselves carry only fn pointers, so this side table
//! is where a signature lives. Codegen emits one [`MethodMeta`] registration
//! per user `def` from facts it already has (that method's `Params`, its
//! `def` node's span, its alias source); the table is populated once from
//! generated `main()` and read-only afterwards -- the same posture as the
//! class registry and the constant store.

use crate::FMap;
use crate::{ClassId, RubyValue, Symbol};
use parking_lot::RwLock;
use std::sync::{Arc, LazyLock};

/// One parameter's kind, matching the leading symbol Ruby's `#parameters`
/// emits. A `post` (required arg after a splat) is a `Req` placed after the
/// `Rest`, so it needs no separate kind -- codegen emits the entries already
/// in Ruby's canonical order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Req,
    Opt,
    Rest,
    KeyReq,
    Key,
    KeyRest,
    Block,
}

impl ParamKind {
    /// The inverse of [`ParamKind::tag`] -- how a `Proc`'s own parameter
    /// metadata spells the same kinds, which is the form a RUNTIME-defined
    /// body's signature arrives in.
    fn from_tag(tag: &str) -> Option<ParamKind> {
        Some(match tag {
            "req" => ParamKind::Req,
            "opt" => ParamKind::Opt,
            "rest" => ParamKind::Rest,
            "keyreq" => ParamKind::KeyReq,
            "key" => ParamKind::Key,
            "keyrest" => ParamKind::KeyRest,
            "block" => ParamKind::Block,
            _ => return None,
        })
    }

    fn tag(self) -> &'static str {
        match self {
            ParamKind::Req => "req",
            ParamKind::Opt => "opt",
            ParamKind::Rest => "rest",
            ParamKind::KeyReq => "keyreq",
            ParamKind::Key => "key",
            ParamKind::KeyRest => "keyrest",
            ParamKind::Block => "block",
        }
    }
}

pub type Descriptor = Vec<(ParamKind, Option<String>)>;

/// Which of a class's two method tables a row describes. `Foo#bar` and
/// `Foo.bar` are different methods that happen to share a name, so they need
/// to be keyed apart.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum MethodKind {
    Instance,
    Singleton,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MethodKey {
    class: u32,
    kind: MethodKind,
    name: Symbol,
}

/// One method's compile-time facts, and its own builder: generated code names
/// the method, adds only the facts that `def` actually has, and registers it.
///
/// ```ignore
/// zeo_rt::MethodMeta::instance(7, "greet")
///     .with_params(vec![(zeo_rt::ParamKind::Req, Some("sound".to_string()))])
///     .defined_at("animal.rb", 2)
///     .register();
///
/// zeo_rt::MethodMeta::instance(7, "yell").aliased_from("greet").register();
/// ```
pub struct MethodMeta {
    key: MethodKey,
    params: Descriptor,
    source: Option<(&'static str, u32)>,
    original_name: Option<Symbol>,
}

impl MethodMeta {
    /// A row for `class`'s instance method `name` (`def name`).
    pub fn instance(class: u32, name: &str) -> MethodMeta {
        MethodMeta::keyed(class, MethodKind::Instance, name)
    }

    /// A row for `class`'s singleton method `name` (`def self.name`).
    pub fn singleton(class: u32, name: &str) -> MethodMeta {
        MethodMeta::keyed(class, MethodKind::Singleton, name)
    }

    fn keyed(class: u32, kind: MethodKind, name: &str) -> MethodMeta {
        MethodMeta {
            key: MethodKey {
                class,
                kind,
                name: Symbol::intern(name),
            },
            params: Descriptor::new(),
            source: None,
            original_name: None,
        }
    }

    /// The signature, already in Ruby's canonical `#parameters` order.
    pub fn with_params(mut self, params: Descriptor) -> MethodMeta {
        self.params = params;
        self
    }

    /// Where the `def` keyword sits. `file` is a literal baked into the
    /// binary; `line` is 1-based, as Ruby reports it.
    pub fn defined_at(mut self, file: &'static str, line: u32) -> MethodMeta {
        self.source = Some((file, line));
        self
    }

    /// The name this method was born under, when it reached `name` through an
    /// `alias`/`alias_method`. Chains are pre-resolved by the compiler, so
    /// this is always the ORIGINAL, never an intermediate alias.
    pub fn aliased_from(mut self, original: &str) -> MethodMeta {
        self.original_name = Some(Symbol::intern(original));
        self
    }

    pub fn register(self) {
        META.write().insert(self.key, Arc::new(self));
    }

    pub fn params(&self) -> &Descriptor {
        &self.params
    }

    pub fn source(&self) -> Option<(&'static str, u32)> {
        self.source
    }

    /// The pre-alias name, or this method's own name when it is not an alias
    /// -- what `Method#original_name` answers either way.
    pub fn original_name(&self) -> Symbol {
        self.original_name.unwrap_or(self.key.name)
    }
}

/// One method's compile-time facts as a CONST-constructible row, so codegen
/// emits a single static table per program instead of a `MethodMeta` builder
/// chain per method. The `const fn` builders keep the chain's
/// omit-absent-facts shape (`MetaRow::inst(7, "greet").params(..).at(..)`);
/// `register_meta_rows` folds a table into the same store
/// `MethodMeta::register` fills.
pub struct MetaRow {
    singleton: bool,
    class: u32,
    name: &'static str,
    params: &'static [(ParamKind, Option<&'static str>)],
    source: Option<(&'static str, u32)>,
    aliased_from: Option<&'static str>,
}

impl MetaRow {
    pub const fn inst(class: u32, name: &'static str) -> MetaRow {
        MetaRow {
            singleton: false,
            class,
            name,
            params: &[],
            source: None,
            aliased_from: None,
        }
    }

    pub const fn sing(class: u32, name: &'static str) -> MetaRow {
        MetaRow {
            singleton: true,
            ..MetaRow::inst(class, name)
        }
    }

    pub const fn params(mut self, params: &'static [(ParamKind, Option<&'static str>)]) -> MetaRow {
        self.params = params;
        self
    }

    pub const fn at(mut self, file: &'static str, line: u32) -> MetaRow {
        self.source = Some((file, line));
        self
    }

    pub const fn alias(mut self, original: &'static str) -> MetaRow {
        self.aliased_from = Some(original);
        self
    }
}

/// The reflection rows of every BODY of a redefined method, held UNregistered
/// -- see [`install_redef_meta`].
static REDEF_METAS: std::sync::OnceLock<&'static [MetaRow]> = std::sync::OnceLock::new();

/// Hand the redefinition-timeline rows over. They are not registered here:
/// each one is registered by the install at its own body's position.
pub fn seed_redef_metas(rows: &'static [MetaRow]) {
    let _ = REDEF_METAS.set(rows);
}

/// Register one redefinition-timeline row, at the position its body installs
/// at. Out of range is impossible from emitted code (the index comes from the
/// same table) and is ignored rather than trusted.
pub fn install_redef_meta(idx: usize) {
    let Some(r) = REDEF_METAS.get().and_then(|rows| rows.get(idx)) else {
        return;
    };
    register_meta_rows(std::slice::from_ref(r));
}

pub fn register_meta_rows(rows: &'static [MetaRow]) {
    let mut map = META.write();
    for r in rows {
        let key = MethodKey {
            class: r.class,
            kind: if r.singleton {
                MethodKind::Singleton
            } else {
                MethodKind::Instance
            },
            name: Symbol::intern(r.name),
        };
        let meta = MethodMeta {
            key,
            params: r
                .params
                .iter()
                .map(|(k, n)| (*k, n.map(str::to_string)))
                .collect(),
            source: r.source,
            original_name: r.aliased_from.map(Symbol::intern),
        };
        map.insert(key, Arc::new(meta));
    }
}

static META: LazyLock<RwLock<FMap<MethodKey, Arc<MethodMeta>>>> =
    LazyLock::new(|| RwLock::new(FMap::default()));

/// The signature of a body installed on ONE OBJECT -- `def obj.m(x, y = 1)`,
/// `obj.define_singleton_method`, `def SOME_ARRAY.pick` -- keyed by the same
/// heap identity the method itself is stored under. It cannot go in [`META`]:
/// that table is keyed by class, and a per-object singleton would then claim to
/// describe every instance of the object's class.
static SINGLETON_PARAMS: LazyLock<RwLock<FMap<usize, FMap<Symbol, SingletonMeta>>>> =
    LazyLock::new(|| RwLock::new(FMap::default()));

/// What a per-object singleton records: the same two facts a class-keyed row
/// carries, minus the ones only a class has.
#[derive(Clone)]
struct SingletonMeta {
    params: Descriptor,
    source: Option<(&'static str, u32)>,
}

/// A `Proc`'s parameter metadata as a method [`Descriptor`]. Codegen records a
/// proc's kinds in Ruby's canonical (lambda) spelling, which is the spelling a
/// method reports, so the two need no translation beyond the tag.
pub(crate) fn descriptor_from_proc(body: &crate::RProc) -> Descriptor {
    body.parameters()
        .iter()
        .filter_map(|p| Some((ParamKind::from_tag(p.kind)?, p.name.map(str::to_string))))
        .collect()
}

/// Record what a runtime `def obj.name` / `define_singleton_method` installed on
/// the object with heap identity `key`. See [`SINGLETON_PARAMS`].
pub(crate) fn record_singleton_params(key: usize, name: Symbol, body: &crate::RProc) {
    SINGLETON_PARAMS.write().entry(key).or_default().insert(
        name,
        SingletonMeta {
            params: descriptor_from_proc(body),
            // Where the body was written -- `def obj.m` reports it like any
            // other Ruby definition.
            source: body.location(),
        },
    );
}

/// Record what a runtime `define_method` / `class_eval`-`def` installed on
/// `class`. Keyed like any compiled `def`, so every reader reaches it already.
pub(crate) fn record_runtime_params(
    class: ClassId,
    kind: MethodKind,
    name: Symbol,
    body: &crate::RProc,
) {
    let key = MethodKey {
        class: class.0,
        kind,
        name,
    };
    let meta = MethodMeta {
        key,
        params: descriptor_from_proc(body),
        // Where the BODY was written. A runtime `def` and a `define_method`
        // both arrive as a proc that already knows its own location, and
        // dropping it here was why every method of a runtime-minted class --
        // `Class.new { def m; end }`, a subclass of one, a `Data.define`
        // subclass -- answered `nil` for `source_location` while its
        // `parameters` and `arity` were right.
        source: body.location(),
        original_name: None,
    };
    META.write().insert(key, Arc::new(meta));
}

/// The signature a per-object singleton on `recv` carries, if the runtime
/// installed one there. Asked BEFORE the class-keyed tables, because a
/// `def obj.m` shadows whatever the object's class says about `m`.
fn singleton_descriptor(recv: &RubyValue, name: Symbol) -> Option<Descriptor> {
    Some(singleton_meta(recv, name)?.params)
}

/// The whole per-object row for `name` on `recv`, if the runtime installed
/// one there.
fn singleton_meta(recv: &RubyValue, name: Symbol) -> Option<SingletonMeta> {
    let key = crate::runtime_meta::value_identity(recv)?;
    SINGLETON_PARAMS.read().get(&key)?.get(&name).cloned()
}

/// The row for `name` as resolved on `class`, walking the ancestry so an
/// inherited method resolves against the ancestor that defined it (matching
/// dispatch). `None` when the compiler registered nothing -- a builtin, a
/// `define_method`, or a name that simply isn't there.
pub fn lookup(class: ClassId, kind: MethodKind, name: Symbol) -> Option<Arc<MethodMeta>> {
    // A class method an `extend` supplied is really the module's INSTANCE
    // method, and that is the only key its row was ever registered under.
    if kind == MethodKind::Singleton
        && let Some(module) = crate::dispatch::class_method_extend_source(class, name)
    {
        return lookup(module, MethodKind::Instance, name);
    }
    let map = META.read();
    crate::dispatch::ancestors_of_value(class)
        .iter()
        .find_map(|&anc| {
            map.get(&MethodKey {
                class: anc.0,
                kind,
                name,
            })
            .cloned()
        })
}

/// The parameter descriptor for `name` on `class`, as reached through `recv`
/// (`None` for an UnboundMethod, which has no receiver). A per-object singleton
/// is asked for first, since `def obj.m` shadows whatever the object's class
/// says about `m`. Struct/Data member accessors are dispatched dynamically with
/// no registration of their own, so their shape is derived from the struct meta
/// instead.
fn descriptor_of(
    snap: Option<&Arc<MethodMeta>>,
    recv: Option<&RubyValue>,
    class: ClassId,
    kind: MethodKind,
    name: Symbol,
) -> Option<Descriptor> {
    if let Some(d) = recv.and_then(|r| singleton_descriptor(r, name)) {
        return Some(d);
    }
    // A handle that captured its row reflects through THAT row. `lookup`
    // answers the row that is live NOW, which is the wrong answer for a
    // `Method` taken before a redefinition -- see `RMethod::meta`.
    if let Some(meta) = snap {
        return Some(meta.params.clone());
    }
    if let Some(meta) = lookup(class, kind, name) {
        return Some(meta.params.clone());
    }
    crate::builtins::rstruct::accessor_params(class, name)
}

/// `Method#arity`: CRuby's signed count -- the number of mandatory parameters
/// (required positionals + post + one if any required keyword), negated and
/// decremented by one when the method takes a variable count (an optional
/// positional, a rest, or a keyword-rest; optional keywords alone do NOT flip
/// the sign). `None` for a method with no registered descriptor (a builtin),
/// letting the caller fall back to its `-1` catch-all.
pub fn arity(
    snap: Option<&Arc<MethodMeta>>,
    recv: Option<&RubyValue>,
    class: ClassId,
    kind: MethodKind,
    name: Symbol,
) -> Option<i64> {
    if let Some(d) = descriptor_of(snap, recv, class, kind, name) {
        return Some(arity_of(&d));
    }
    builtin_arity(class, kind, name)
}

/// A builtin (C-defined) method has no `Params` descriptor -- its arity is the
/// argc declared in its class's `ruby_class!`/`ruby_module!` definition. Walk
/// the receiver's ancestry so an inherited builtin resolves against its owner.
pub(crate) fn builtin_arity(class: ClassId, kind: MethodKind, name: Symbol) -> Option<i64> {
    let n = name.name();
    let n = n.as_str();
    crate::dispatch::ancestors_of_value(class)
        .iter()
        .find_map(|&anc| {
            match kind {
                MethodKind::Instance => crate::builtins::class_arity_table(anc),
                MethodKind::Singleton => crate::builtins::class_method_arity_table(anc),
            }
            .and_then(|f| f(n))
        })
        .or_else(|| {
            // Same extend route the signature lookup takes, so a row cannot
            // report a signature from one place and an arity from another.
            let m = (kind == MethodKind::Singleton)
                .then(|| extend_source(class, name))
                .flatten()?;
            crate::builtins::class_arity_table(m)?(n)
        })
}

/// The signature a native row SPELLS (`params "..."` in the DSL), or `None`
/// for a row that spells none -- which is still most of them.
///
/// The walk stops at the first ancestor whose table OWNS the name, and answers
/// that ancestor's spelling even when it has none. Continuing past the owner
/// would let a further ancestor's signature stand in for a row that never
/// declared one, which is a wrong answer rather than a missing one.
fn builtin_params(class: ClassId, kind: MethodKind, name: Symbol) -> Option<Descriptor> {
    let n = name.name();
    let n = n.as_str();
    // An explicit loop, NOT `find_map`: a closure returning `None` makes
    // `find_map` keep walking, which is exactly the wrong move once the owner
    // is found. `Enumerable#to_set` spells its signature and `Range#to_set`
    // does not, so continuing past Range answered Enumerable's spelling for
    // ruby's own separate row.
    for &anc in crate::dispatch::ancestors_of_value(class).iter() {
        let (arity, params) = match kind {
            MethodKind::Instance => (
                crate::builtins::class_arity_table(anc),
                crate::builtins::class_params_table(anc),
            ),
            MethodKind::Singleton => (
                crate::builtins::class_method_arity_table(anc),
                crate::builtins::class_method_params_table(anc),
            ),
        };
        let Some(arity) = arity else { continue };
        if arity(n).is_none() {
            continue;
        }
        // This ancestor owns the row. Its answer is the answer, spelled or not.
        return params?(n).map(to_descriptor);
    }
    // A class method an `extend` supplies is an INSTANCE row seated in the
    // singleton chain, which the walk above cannot see -- `SecureRandom.hex`
    // is `Random::Formatter#hex`, and `Method#owner` already says so.
    if kind == MethodKind::Singleton
        && let Some(m) = extend_source(class, name)
        && let Some(table) = crate::builtins::class_params_table(m)
    {
        return table(n).map(to_descriptor);
    }
    None
}

fn to_descriptor(rows: crate::builtins::ParamRows) -> Descriptor {
    rows.iter()
        .map(|(k, name)| (*k, name.map(str::to_string)))
        .collect()
}

/// The module an `extend` supplies a class method from, asked only when the
/// ordinary walk missed. Behind the overlay gate, because an extend is a
/// runtime fact and a program that never made one must not pay for the probe.
fn extend_source(class: ClassId, name: Symbol) -> Option<ClassId> {
    crate::runtime_meta::is_live()
        .then(|| crate::dispatch::class_method_extend_source(class, name))
        .flatten()
}

/// CRuby's signed arity for one descriptor. Required positionals (a post arg is
/// emitted as a trailing `Req`, so it counts here too) form the mandatory base.
/// An optional positional or a rest makes the method variadic. Keywords act as
/// one unit: ANY required keyword adds a single mandatory slot (the keyword hash
/// is required) and keeps the arity fixed; otherwise an optional keyword or a
/// keyword-rest makes it variadic. A block parameter never affects arity.
pub(crate) fn arity_of(d: &[(ParamKind, Option<String>)]) -> i64 {
    let mut mandatory = d.iter().filter(|(k, _)| *k == ParamKind::Req).count() as i64;
    let mut variadic = d
        .iter()
        .any(|(k, _)| matches!(k, ParamKind::Opt | ParamKind::Rest));
    if d.iter().any(|(k, _)| *k == ParamKind::KeyReq) {
        mandatory += 1;
    } else if d
        .iter()
        .any(|(k, _)| matches!(k, ParamKind::Key | ParamKind::KeyRest))
    {
        variadic = true;
    }
    if variadic {
        -(mandatory + 1)
    } else {
        mandatory
    }
}

/// `Method#parameters`: the array of `[kind, name]` pairs (an anonymous
/// rest/keyrest/block is a one-element `[kind]`). A builtin has no descriptor,
/// so its shape is synthesized from its declared arity the way CRuby reports a
/// C function: `n >= 0` mandatory anonymous slots, or `-n-1` of them followed
/// by a rest.
pub fn parameters(
    snap: Option<&Arc<MethodMeta>>,
    recv: Option<&RubyValue>,
    class: ClassId,
    kind: MethodKind,
    name: Symbol,
) -> Option<RubyValue> {
    let d = descriptor_of(snap, recv, class, kind, name)
        .or_else(|| builtin_params(class, kind, name))
        .or_else(|| builtin_arity(class, kind, name).map(anonymous_descriptor))?;
    let pairs = d
        .into_iter()
        .map(|(kind, pname)| {
            let mut entry = vec![RubyValue::Symbol(Symbol::intern(kind.tag()))];
            if let Some(n) = pname {
                entry.push(RubyValue::Symbol(Symbol::intern(&n)));
            }
            RubyValue::Array(crate::array_new(entry))
        })
        .collect();
    Some(RubyValue::Array(crate::array_new(pairs)))
}

/// `Method#source_location` / `UnboundMethod#source_location`: the
/// `[file, line]` codegen baked from the `def` keyword's span, or `nil` for a
/// method with no Ruby source here -- a builtin, or a body this runtime
/// synthesized. CRuby answers `nil` for its own C methods the same way.
pub fn source_location(
    snap: Option<&Arc<MethodMeta>>,
    recv: Option<&RubyValue>,
    class: ClassId,
    kind: MethodKind,
    name: Symbol,
) -> RubyValue {
    let Some((file, line)) = source_pair(snap, recv, class, kind, name) else {
        return RubyValue::Nil;
    };
    RubyValue::Array(crate::array_new(vec![
        RubyValue::Str(crate::string_new(file.to_string())),
        RubyValue::Int(line as i64),
    ]))
}

/// [`source_location`]'s raw pair -- exactly the shape `ProcData::location`
/// wants, so `Method#to_proc` can carry its method's location.
pub(crate) fn source_pair(
    snap: Option<&Arc<MethodMeta>>,
    recv: Option<&RubyValue>,
    class: ClassId,
    kind: MethodKind,
    name: Symbol,
) -> Option<(&'static str, u32)> {
    // A per-object singleton shadows whatever the object's class says, and
    // lives in its own table -- the same precedence `descriptor_of` applies.
    if let Some(m) = recv.and_then(|r| singleton_meta(r, name)) {
        return m.source;
    }
    match snap {
        Some(m) => m.source,
        None => lookup(class, kind, name).and_then(|m| m.source),
    }
}

/// The pieces of CRuby's `method_inspect` (proc.c), assembled by
/// [`Inspect::render`]. Both `Method` and `UnboundMethod` fill this in and
/// share the one formatter:
///
/// ```text
/// #<Method: Sub(Base)#greet() f.rb:1>   inherited -- home, then the owner
/// #<Method: A#y(x)(q) f.rb:5>           an alias -- its birth name in parens
/// #<Method: Q.z(x) f.rb:9>              a class method -- a `.`, not a `#`
/// #<Method: #<Object:0x…>.only() f.rb:17>   a per-object singleton
/// #<Method: Array#each()>               native -- placeholders, no location
/// #<UnboundMethod: M#hi() f.rb:1>       unbound -- owner only, never qualified
/// ```
pub(crate) struct Inspect<'a> {
    /// `"Method"` or `"UnboundMethod"`.
    pub label: &'a str,
    /// How the lookup's starting point prints -- a class name, or a
    /// receiver's own `inspect` for a per-object singleton.
    pub home: String,
    /// The defining class, when it differs from `home` and so needs naming.
    pub owner: Option<String>,
    /// `'.'` for a singleton method, `'#'` for an instance method.
    pub separator: char,
    pub name: Symbol,
    /// The pre-alias name, when this method reached `name` through an alias.
    pub original: Option<Symbol>,
    pub params: Descriptor,
    pub source: Option<(&'static str, u32)>,
}

impl Inspect<'_> {
    pub(crate) fn render(&self) -> String {
        let mut out = format!("#<{}: {}", self.label, self.home);
        if let Some(owner) = &self.owner {
            out.push_str(&format!("({owner})"));
        }
        out.push(self.separator);
        out.push_str(&self.name.name());
        if let Some(original) = self.original {
            out.push_str(&format!("({})", original.name()));
        }
        out.push_str(&render_params(&self.params));
        if let Some((file, line)) = self.source {
            out.push_str(&format!(" {file}:{line}"));
        }
        out.push('>');
        out
    }
}

/// The parenthesized signature: each parameter as Ruby spells it in a `def`.
/// An entry with NO name is a native method's placeholder (see
/// [`anonymous_descriptor`]), which CRuby prints as a bare `_` or `*`.
///
/// An ANONYMOUS forwarding slot is named for its own sigil (`*`/`**`/`&`), and
/// ruby renders those three specially -- oracle-verified over every
/// combination: a signature that is nothing but the whole triple, or nothing
/// but an anonymous block, is `(...)`; otherwise an anonymous block prints
/// `...`, and an anonymous keyrest directly after an anonymous rest is left
/// out entirely.
fn render_params(params: &Descriptor) -> String {
    fn is(e: &(ParamKind, Option<String>), kind: ParamKind, sigil: &str) -> bool {
        e.0 == kind && e.1.as_deref() == Some(sigil)
    }
    let triple = params.len() == 3
        && is(&params[0], ParamKind::Rest, "*")
        && is(&params[1], ParamKind::KeyRest, "**")
        && is(&params[2], ParamKind::Block, "&");
    if triple || (params.len() == 1 && is(&params[0], ParamKind::Block, "&")) {
        return "(...)".to_string();
    }
    let one = |(kind, name): &(ParamKind, Option<String>)| match (kind, name.as_deref()) {
        (ParamKind::Req, None) => "_".to_string(),
        (ParamKind::Req, Some(n)) => n.to_string(),
        (ParamKind::Opt, n) => format!("{}=...", n.unwrap_or("_")),
        (ParamKind::Rest, Some("*")) => "*".to_string(),
        (ParamKind::Rest, n) => format!("*{}", n.unwrap_or("")),
        (ParamKind::KeyReq, n) => format!("{}:", n.unwrap_or("_")),
        (ParamKind::Key, n) => format!("{}: ...", n.unwrap_or("_")),
        (ParamKind::KeyRest, Some("**")) => "**".to_string(),
        (ParamKind::KeyRest, n) => format!("**{}", n.unwrap_or("")),
        (ParamKind::Block, n) => format!("&{}", n.unwrap_or("")),
    };
    let mut out: Vec<String> = Vec::new();
    for (i, e) in params.iter().enumerate() {
        if is(e, ParamKind::KeyRest, "**") && i > 0 && is(&params[i - 1], ParamKind::Rest, "*") {
            continue;
        }
        out.push(if is(e, ParamKind::Block, "&") {
            "...".to_string()
        } else {
            one(e)
        });
    }
    format!("({})", out.join(", "))
}

/// The signature to PRINT for `class`'s `name`: the compiler's descriptor when
/// there is one, else the placeholders derived from a native method's arity.
pub(crate) fn printable_params(
    snap: Option<&Arc<MethodMeta>>,
    recv: Option<&RubyValue>,
    class: ClassId,
    kind: MethodKind,
    name: Symbol,
) -> Descriptor {
    descriptor_of(snap, recv, class, kind, name)
        .or_else(|| builtin_arity(class, kind, name).map(anonymous_descriptor))
        .unwrap_or_default()
}

/// Where the `def` was written, for the ` file:line` tail of `#inspect`.
pub(crate) fn source_of(
    class: ClassId,
    kind: MethodKind,
    name: Symbol,
) -> Option<(&'static str, u32)> {
    lookup(class, kind, name)?.source
}

/// `Method#original_name`: the name this method was born under -- its own
/// unless an `alias` renamed it.
pub fn original_name(class: ClassId, kind: MethodKind, name: Symbol) -> Symbol {
    alias_origin(class, kind, name).unwrap_or(name)
}

/// The birth name only when it DIFFERS from the name in hand -- the
/// `A#y(x)` qualifier `#inspect` adds for an alias.
///
/// An alias of a BUILTIN (`alias_method :raise!, :raise`) has no body to
/// clone and so no row here, only a name indirection in the class registry;
/// that indirection is the birth name just the same.
pub(crate) fn alias_origin(class: ClassId, kind: MethodKind, name: Symbol) -> Option<Symbol> {
    let origin = match lookup(class, kind, name) {
        Some(meta) => meta.original_name,
        None => crate::dispatch::alias_target(class, name),
    };
    origin.filter(|&origin| origin != name)
}

/// The nameless descriptor CRuby reports for a method defined in C, derived
/// from its arity alone: `2` is `[[:req], [:req]]`, `-1` is `[[:rest]]`, and
/// `-2` is `[[:req], [:rest]]`.
pub(crate) fn anonymous_descriptor(arity: i64) -> Descriptor {
    let (required, rest) = if arity >= 0 {
        (arity, false)
    } else {
        (-arity - 1, true)
    };
    let mut d: Descriptor = (0..required).map(|_| (ParamKind::Req, None)).collect();
    if rest {
        d.push((ParamKind::Rest, None));
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arity_counts_mandatory_and_detects_variadic() {
        let req = |n: &str| (ParamKind::Req, Some(n.to_string()));
        assert_eq!(arity_of(&[]), 0);
        assert_eq!(arity_of(&[req("a"), req("b")]), 2);
        assert_eq!(
            arity_of(&[req("a"), (ParamKind::Opt, Some("b".into()))]),
            -2
        );
        assert_eq!(arity_of(&[(ParamKind::Rest, None)]), -1);
        // A required keyword adds one mandatory slot and stays fixed; an
        // optional one only makes the method variadic.
        assert_eq!(
            arity_of(&[req("a"), (ParamKind::KeyReq, Some("k".into()))]),
            2
        );
        assert_eq!(
            arity_of(&[req("a"), (ParamKind::Key, Some("k".into()))]),
            -2
        );
        // A block parameter never counts.
        assert_eq!(
            arity_of(&[req("a"), (ParamKind::Block, Some("b".into()))]),
            1
        );
    }

    /// A proc that knows where it was written, the way a compiled `def` body
    /// or a `define_method` block arrives.
    fn located_proc(file: &'static str, line: u32) -> crate::RProc {
        crate::rproc::ProcBuilder::from_rust(
            |_recv, _args, _block| Ok(RubyValue::Nil),
            RubyValue::Nil,
            -1,
            false,
        )
        .location(file, line)
        .build()
    }

    /// A RUNTIME-installed method carries the location of the body it was
    /// installed from.
    ///
    /// `record_runtime_params` held the proc and built its row with
    /// `source: None`, so every method of a runtime-minted class answered nil
    /// for `source_location` while its `parameters` and `arity` were right.
    /// The whole family took this one path: `Class.new { def m; end }`, a
    /// subclass of one, a `define_method` body, and every method of a
    /// `Data.define` subclass -- which is why a Data subclass's backtrace
    /// frame could not be named either.
    #[test]
    fn a_runtime_installed_method_records_where_its_body_was_written() {
        // A REAL class id: `lookup` resolves through the ancestry, so a
        // synthetic id has no chain to find its own row on.
        let class = zeo_abi::STRING_CLASS;
        let name = Symbol::intern("runtime_installed");
        record_runtime_params(
            class,
            MethodKind::Instance,
            name,
            &located_proc("mint.rb", 12),
        );
        assert_eq!(
            source_pair(None, None, class, MethodKind::Instance, name),
            Some(("mint.rb", 12))
        );
    }

    /// A body with no location -- a Rust-implemented proc -- records none,
    /// and the row still answers for `parameters`. `nil` is the right answer
    /// there, and it is what ruby gives a C-defined method.
    #[test]
    fn a_bodiless_proc_records_no_location() {
        let class = zeo_abi::STRING_CLASS;
        let name = Symbol::intern("no_location");
        record_runtime_params(
            class,
            MethodKind::Instance,
            name,
            &crate::RProc::new(|_| Ok(RubyValue::Nil)),
        );
        assert_eq!(
            source_pair(None, None, class, MethodKind::Instance, name),
            None
        );
    }

    /// A per-object singleton keeps its own row, and that row carries a
    /// location too. It cannot live in `META`, which is keyed by class: a
    /// `def obj.m` would then claim to describe every instance of the
    /// object's class.
    #[test]
    fn a_per_object_singleton_records_its_own_location() {
        let recv = RubyValue::Str(crate::string_new("receiver".to_string()));
        let key = crate::runtime_meta::value_identity(&recv).expect("a heap value has identity");
        let name = Symbol::intern("only_on_this_one");
        record_singleton_params(key, name, &located_proc("singleton.rb", 7));
        // Asked WITH the receiver it answers; the class-keyed lookup for the
        // same name does not, which is the separation the table exists for.
        assert_eq!(
            source_pair(
                None,
                Some(&recv),
                zeo_abi::STRING_CLASS,
                MethodKind::Instance,
                name
            ),
            Some(("singleton.rb", 7))
        );
        assert_eq!(
            source_pair(
                None,
                None,
                zeo_abi::STRING_CLASS,
                MethodKind::Instance,
                name
            ),
            None
        );
    }

    /// A per-object singleton SHADOWS the class-keyed row for the same name,
    /// on the source channel exactly as it already does on the parameter one.
    #[test]
    fn a_singleton_row_shadows_the_class_row() {
        let class = zeo_abi::ARRAY_CLASS;
        let name = Symbol::intern("shadowed");
        record_runtime_params(
            class,
            MethodKind::Instance,
            name,
            &located_proc("class_row.rb", 1),
        );
        let recv = RubyValue::Array(crate::array_new(vec![]));
        let key = crate::runtime_meta::value_identity(&recv).expect("a heap value has identity");
        record_singleton_params(key, name, &located_proc("singleton_row.rb", 2));
        assert_eq!(
            source_pair(None, Some(&recv), class, MethodKind::Instance, name),
            Some(("singleton_row.rb", 2)),
            "the object's own row must win"
        );
    }

    /// A CAPTURED row wins over the live table -- the `Method` handle
    /// snapshot. Re-recording the same key must not change what a handle
    /// taken earlier answers.
    #[test]
    fn a_captured_row_outranks_a_later_redefinition() {
        let class = zeo_abi::HASH_CLASS;
        let name = Symbol::intern("redefined_later");
        record_runtime_params(
            class,
            MethodKind::Instance,
            name,
            &located_proc("first.rb", 3),
        );
        let captured = lookup(class, MethodKind::Instance, name).expect("the row was recorded");
        record_runtime_params(
            class,
            MethodKind::Instance,
            name,
            &located_proc("second.rb", 9),
        );
        assert_eq!(
            source_pair(Some(&captured), None, class, MethodKind::Instance, name),
            Some(("first.rb", 3)),
            "a captured row must not follow the redefinition"
        );
        assert_eq!(
            source_pair(None, None, class, MethodKind::Instance, name),
            Some(("second.rb", 9)),
            "and the live table must"
        );
    }

    #[test]
    fn a_builtin_arity_becomes_an_anonymous_descriptor() {
        let kinds = |d: Descriptor| d.into_iter().map(|(k, n)| (k.tag(), n)).collect::<Vec<_>>();
        assert!(anonymous_descriptor(0).is_empty());
        assert_eq!(
            kinds(anonymous_descriptor(2)),
            [("req", None), ("req", None)]
        );
        assert_eq!(kinds(anonymous_descriptor(-1)), [("rest", None)]);
        assert_eq!(
            kinds(anonymous_descriptor(-3)),
            [("req", None), ("req", None), ("rest", None)]
        );
    }
}
