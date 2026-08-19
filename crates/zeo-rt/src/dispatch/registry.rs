//! The class registry: `ClassEntry` (per-class method/ancestry tables),
//! `FlatHit` (one flattened dispatch answer), `ClassRegistry` itself, and
//! the process-wide `REGISTRY` install. Everything else in `dispatch`
//! queries what lives here.

use super::*;

pub(super) struct ClassEntry {
    /// The Ruby-visible, fully-qualified name (`"Store::Item"`) -- what
    /// `Class#name`/`#to_s`/`puts Widget` print, and what `NoMethodError`
    /// messages cite (retiring the class-id-in-the-message
    /// approximation).
    pub(super) name: String,
    pub(super) is_module: bool,
    /// A `refine Target do ... end` holder: a module in every respect except
    /// that its own `.class` answers `Refinement`, which is how a refined
    /// `Method#owner` identifies itself. Carries `(refining module, refined
    /// target)` -- what `Module#refinements` and `Refinement#target` answer,
    /// and what renders the `#<refinement:String@M>` name. Marked after
    /// registration, since only the compiler knows which modules a `refine`
    /// block minted.
    pub(super) refinement_of: Option<(ClassId, ClassId)>,
    /// The full, already-linearized MRO (this class first, then prepends/
    /// includes/superclass in resolution order) -- computed at COMPILE time
    /// by `zeo::analyze::mro::compute_ancestors` and baked in as a
    /// literal list by `ruby_class!`'s generated `__register`. Method
    /// dispatch itself never needs to walk this (every reachable method is
    /// already MATERIALIZED directly onto this class -- see the plan's Part
    /// 6), but `is_a`/rescue-by-class matching does.
    pub(super) ancestors: Vec<ClassId>,
    /// The modules this class's BODY `extend`ed (`module M; extend self; end`,
    /// `class C; extend Forwardable; end`). Deliberately absent from
    /// `ancestors`, which linearizes instance-method resolution: an extended
    /// module joins the class's SINGLETON chain instead, so it answers
    /// `C.is_a?(M)` and shows up in `C.singleton_class.ancestors` while
    /// leaving `C.new.is_a?(M)` false. The per-OBJECT form of the same fact
    /// lives in `runtime_meta`'s identity-keyed map; this half is compile-time
    /// known, so it is baked in beside the ancestry it deliberately is not
    /// part of.
    pub(super) extends: Vec<ClassId>,
    pub(super) methods: FMap<Symbol, MethodImpl>,
    /// Methods added by reopening a BUILTIN class -- keyed off
    /// the receiver's `class_id()` with no ancestor walk needed (the only
    /// reopenable builtins are leaf value classes; Object/module reopens are
    /// rejected at zeo compile time). Empty for every user class, whose
    /// methods live in `methods` above. Keyed `(box_id, name)`: a builtin
    /// reopened INSIDE a `Ruby::Box` registers its methods under that box's
    /// id, and dispatch probes `(caller's box, name)` then `(0, name)` -- the
    /// AOT translation of CRuby's `cme->def->box` stamping, root reopens
    /// visible everywhere.
    ///
    /// KNOWN DIVERGENCE (box-only, bundler-irrelevant): this `(box_id, name)`
    /// dimension covers INSTANCE methods on a reopened builtin. A per-box
    /// SINGLETON method (`class String; def self.count; end` inside a box) and
    /// a per-box CLASS VARIABLE / class-ivar on a shared builtin are keyed by
    /// `ClassId` alone (see `cvars`/`civars`), which for a shared builtin is
    /// the same id in every box -- so, unlike CRuby, they are not isolated
    /// per box. Per-box state on distinct USER classes (their own `ClassId`
    /// per box) and per-box instance-method monkeypatches both work.
    pub(super) value_methods: FMap<(u32, Symbol), ValueMethodFn>,
    /// Just the NAMES in `value_methods`, box dimension collapsed.
    /// `defines_own` and `own_method_visibility` ask "does this class
    /// reopen this name at all", which the keyed map can only answer by
    /// scanning every row -- and `String`/`Array` carry ~200 each, once
    /// per ancestor of every MRO walk. Maintained solely by
    /// `define_value_method`, the one insertion point.
    pub(super) own_value_names: FSet<Symbol>,
    /// The `value_methods` rows this class did NOT define -- a REOPENED
    /// BUILTIN registers its whole FLATTENED table (`String.prepend(Loud)`
    /// puts `Loud#upcase` on `String`), and `super` must not find one of
    /// those at the position it is resuming from or a prepended method's
    /// `super` finds itself and recurses forever. `own_impls` is the object
    /// channel's answer to the same question; this is the value channel's.
    ///
    /// Only the foreign names are recorded, not the own ones: on a reopened
    /// builtin the flattened winner is almost always the class's own def, so
    /// the set is empty for every class that mixes nothing in.
    pub(super) foreign_value_names: FSet<Symbol>,
    /// This class's OWN method implementations -- what `super` resolution
    /// walks. `methods` above is the FLATTENED instance-dispatch set (an
    /// entry's winner can be a prepended module's or an ancestor's copy),
    /// which is exactly wrong for `super`: resuming an MRO walk needs each
    /// position's own contribution, or a prepended method's `super` finds
    /// itself again. Filled three ways: `promote_own_impl` (the common
    /// case -- the flattened winner IS the own def), `define_method_own`
    /// (native exception sets), and `define_super_target_value` (a
    /// dynamic-self bridge for an own def shadowed in its own class's
    /// flattened table).
    pub(super) own_impls: FMap<Symbol, MethodImpl>,
    /// The names in `methods`/`value_methods` that Ruby considers PRIVATE
    /// (`private def x`, and every top-level `def` -- which is a private
    /// method of Object). Dispatch itself ignores this (an implicit-self
    /// call and `send` both reach privates, and codegen enforces the
    /// explicit-receiver rule statically where it can); it exists so
    /// `respond_to?` can skip them, matching CRuby's "the default ignores
    /// private methods" rule. Kernel's own C-implemented privates
    /// (`puts`/`p`/...) aren't here -- they have no registry entry at all
    /// and are special-cased in `responds_to`.
    pub(super) private_methods: FSet<Symbol>,
    /// The names in `methods`/`value_methods` this class marks PROTECTED
    /// (`protected def x`, a bare `protected` section, `protected :x`). Parallel
    /// to `private_methods`; a name in neither set is public. Materialization
    /// stamps the mark onto every descendant that inherits the method, so this
    /// set is authoritative for the class itself (no ancestor walk needed to
    /// answer "is THIS class's `name` protected"). Consumed by the
    /// `protected_*` reflection and the `*_method_defined?` family.
    pub(super) protected_methods: FSet<Symbol>,
    /// The names in `class_methods` this class marks PRIVATE
    /// (`private_class_method :x`, `private_class_method def self.x`). Ruby
    /// has no protected class method and no running class-method default, so
    /// one set is the whole model. Kept apart from `private_methods` because
    /// `Foo.bar` and `Foo#bar` are different methods that share a name.
    pub(super) private_class_methods: FSet<Symbol>,
    /// Names this class's body `undef`'d. Mirrors CRuby, where `undef`
    /// inserts an "undefined" method entry that TERMINATES the lookup
    /// rather than deleting anything -- so an inherited name stops
    /// resolving here while staying live on the ancestor that defined it.
    ///
    /// Dispatch itself needs no check: `mro::materialize_methods` already
    /// refuses to materialize an undef'd name onto this class, and dispatch
    /// only ever reads this class's own table. `respond_to?` is the one
    /// consumer, because it WALKS the ancestors and would otherwise find
    /// the ancestor's still-live definition.
    pub(super) undefined_methods: FSet<Symbol>,
    /// `new -> old` NAME indirections for aliases of INHERITED BUILTIN
    /// methods (`alias_method :raise!, :raise`): the source has no user
    /// `Scope` to clone a body from -- it lives in the static builtin
    /// tables -- so the alias is recorded as a name rewrite instead. The
    /// send miss paths consult this via `alias_target` (closest ancestor
    /// first, so subclasses inherit it through the ordinary MRO walk) and
    /// re-dispatch under `old`. Entries are TERMINAL: the compiler resolves
    /// an alias-of-an-alias before emitting `register_alias`, and
    /// `validate_aliases` raises `NameError` at program start for a source
    /// that resolves nowhere (real Ruby's timing -- the class body
    /// executing).
    pub(super) aliases: FMap<Symbol, Symbol>,
    /// This class's own CLASS methods (`def self.x`, `class << self`,
    /// `extend`) -- reached when a `RubyValue::Class` receiver is sent to
    /// dynamically (`handler.run(...)`, where `handler` holds a class), the
    /// one path where the callee isn't statically known and so can't be a
    /// direct `Foo::__cm_run(...)` call.
    ///
    /// `ValueMethodFn`-shaped like `value_methods`, but with a DROPPED
    /// receiver rather than a passed one (`params::RecvMode`): the emitted
    /// class-method free function takes no receiver parameter.
    ///
    /// No ancestor walk on lookup, for the same reason `methods` needs
    /// none: `analyze::mro::materialize_class_methods` already copies every
    /// inherited class method onto each subclass at compile time, so a hit
    /// here is always this exact class's own entry. That is also what keeps
    /// class-level `@x` storage correct through this path -- each copy
    /// carries its own class id (see `civars`' docs).
    pub(super) class_methods: FMap<Symbol, ValueMethodFn>,
    /// [`ClassEntry::aliases`]'s singleton-side twin: `new -> old` name
    /// indirections for an alias written inside `class << self` whose source is
    /// a builtin class method rather than a user `def self.x`.
    ///
    /// `class << self; alias [] new; end` is the whole reason -- the
    /// `Klass[...]` constructor shorthand, which rack, rack-test, pry, coderay,
    /// sprockets, warden and omniauth all write. Its source is `Class#new`, a
    /// row in the static builtin class-method table with no `Scope` to clone,
    /// so it records as a rewrite exactly as the instance side does.
    ///
    /// Separate from `aliases` because the two tables are consulted with
    /// different receivers: `aliases` answers for INSTANCES of this class,
    /// this one for the class OBJECT itself.
    pub(super) class_aliases: FMap<Symbol, Symbol>,
    /// Per-POSITION singleton-chain super targets, keyed `(module id,
    /// name)`: one emitted copy of every `extend`ed module's method (winner
    /// AND shadowed -- the flattened `class_methods` above keeps only
    /// winners, which is exactly wrong for `super` the same way `methods`
    /// is vs `own_impls`). `call_singleton_super_target` probes this before
    /// the module's generic bridge, so a sibling-extend chain (`extend A`
    /// then `extend B`, each `super`ing to the next) resolves every hop
    /// with the RECEIVER's context.
    pub(super) singleton_super_targets: FMap<(u32, Symbol), ValueMethodFn>,
    /// The names DEFINED DIRECTLY on this class (not materialized from an
    /// ancestor) -- what `instance_methods(false)` needs, since `methods`
    /// above holds the flattened, fully-materialized set (dispatch's own
    /// requirement -- see `ancestors`' docs). Populated by codegen's
    /// `mark_own` beside `mark_private`. Empty means "unknown/none recorded",
    /// in which case reflection falls back to the materialized set.
    pub(super) own_methods: FSet<Symbol>,
    /// `own_methods`' class-method twin: the names whose `def self.x` is
    /// written HERE, as opposed to materialized down from an ancestor. Only
    /// reflection needs the distinction -- `Cache.method(:open).owner` has to
    /// answer `Store` even though materialization gave `Cache` a copy.
    pub(super) own_class_methods: FSet<Symbol>,
    pub(super) constructor: Option<ConstructorFn>,
    /// The no-`initialize` allocator backing `Class#allocate` -- registered by
    /// `ruby_class!`'s `__register` beside the constructor. `None` for
    /// modules/builtins.
    pub(super) allocator: Option<AllocatorFn>,
    /// Box-0 VALUE-receiver dispatch, flattened: exactly what the per-send
    /// ancestor walk (reopen row, then builtin table, per ancestor) would
    /// find, memoized the first time a dynamic send asks. Lives inside the
    /// frozen registry and is consulted only while `runtime_meta` is dormant
    /// and the caller is box 0, so there is NOTHING to invalidate:
    /// post-install definitions all go through the overlay, whose `is_live`
    /// gate is probed ahead of this on every tier.
    pub(super) flat_value: OnceLock<crate::FMap<Symbol, FlatHit>>,
    /// The class-receiver twin (`File.read`, `Math.sqrt`): the frozen
    /// `class_methods` rows over the builtin class-method table, flattened.
    /// Frozen-layer-only like `flat_value`, but box-free (class methods are
    /// not box-scoped) so it serves every box; the runtime overlay is still
    /// probed ahead of it when live. The label rides along for BUILTIN rows
    /// (`'File.read'` -- see [`FlatHit::frame_label`]); user `def self.x`
    /// rows push their own compiled frames and carry `None`.
    pub(super) flat_class: OnceLock<crate::FMap<Symbol, (ValueMethodFn, Option<&'static str>)>>,
}

/// One flattened dispatch answer: the function, which ancestor supplied it,
/// and whether it came from a builtin table (a value-subclass payload
/// rewraps only builtin hits at its payload root -- reopen rows always run
/// against the boxed receiver, mirroring the walk).
#[derive(Clone, Copy)]
pub(super) struct FlatHit {
    pub(super) f: ValueMethodFn,
    pub(super) owner: ClassId,
    pub(super) builtin: bool,
    /// The `'Owner#name'` backtrace frame this row shows while it runs --
    /// CRuby names the C frames a raise passes through, so every builtin hit
    /// pushes one (see [`crate::frames::synthetic_c_frame`]). `None` for
    /// reopen rows (compiled bodies push their own real frames) and for the
    /// [`NOFRAME`] set.
    pub(super) frame_label: Option<&'static str>,
}

#[derive(Default)]
pub struct ClassRegistry {
    pub(super) entries: FMap<u32, ClassEntry>,
    /// Fully-qualified class NAME -> id, so the runtime can construct an
    /// exception by name (`raise_error("ArgumentError", ...)`) without the
    /// generated program installing a name->constructor factory. Populated by
    /// `register` alongside `entries`. See `construct_exception`.
    pub(super) by_name: FMap<String, u32>,
}

impl ClassRegistry {
    pub fn new() -> ClassRegistry {
        ClassRegistry::default()
    }

    /// Mirrors declaring a class's place in the hierarchy -- called once per
    /// class from that class's generated `__register` (or directly from
    /// generated `main()` for modules/builtins, which have no struct), with
    /// `ancestors` already fully linearized at zeo compile time.
    pub fn register(
        &mut self,
        id: ClassId,
        name: &str,
        is_module: bool,
        ancestors: Vec<ClassId>,
        constructor: Option<ConstructorFn>,
    ) {
        self.by_name.insert(name.to_string(), id.0);
        self.entries.insert(
            id.0,
            ClassEntry {
                name: name.to_string(),
                is_module,
                refinement_of: None,
                ancestors,
                extends: Vec::new(),
                methods: FMap::default(),
                own_impls: FMap::default(),
                value_methods: FMap::default(),
                foreign_value_names: FSet::default(),
                own_value_names: FSet::default(),
                private_methods: FSet::default(),
                protected_methods: FSet::default(),
                private_class_methods: FSet::default(),
                undefined_methods: FSet::default(),
                aliases: FMap::default(),
                class_methods: FMap::default(),
                class_aliases: FMap::default(),
                singleton_super_targets: FMap::default(),
                own_methods: FSet::default(),
                own_class_methods: FSet::default(),
                constructor,
                allocator: None,
                flat_value: OnceLock::new(),
                flat_class: OnceLock::new(),
            },
        );
    }

    /// The flattened box-0 value-dispatch answer for `(id, name)`, building
    /// `id`'s map on first use. Outer `None` means `id` has no registry
    /// entry at all -- the caller must run the ordinary walk; inner `None`
    /// is a genuine miss (proceed to the alias/method_missing tail).
    pub(super) fn flat_value_hit(&self, id: ClassId, name: Symbol) -> Option<Option<FlatHit>> {
        let entry = self.entries.get(&id.0)?;
        let map = entry.flat_value.get_or_init(|| {
            let mut map = crate::FMap::default();
            // A name `undef`'d part-way up the chain must not be flattened in
            // from ABOVE that point -- `Complex` undefines `positive?`, so the
            // `Numeric` row behind it may never reach this table. Collected as
            // the walk descends, so an undef blocks its own ancestor's rows and
            // every one after it, and never the more-derived rows already in.
            let mut blocked: FSet<Symbol> = FSet::default();
            for &anc in entry.ancestors.iter() {
                // Reopen rows first, then the builtin table -- the walk's own
                // per-ancestor order; `or_insert`-style first-wins across
                // ancestors is the walk's most-derived-first rule.
                if let Some(anc_entry) = self.entries.get(&anc.0) {
                    blocked.extend(anc_entry.undefined_methods.iter().copied());
                    for (&(b, sym), &f) in &anc_entry.value_methods {
                        if b == 0 && !blocked.contains(&sym) {
                            map.entry(sym).or_insert(FlatHit {
                                f,
                                owner: anc,
                                builtin: false,
                                frame_label: None,
                            });
                        }
                    }
                }
                if let Some(lookup) = crate::builtins::class_table(anc) {
                    for &n in crate::builtins::class_table_names(anc) {
                        let sym = Symbol::intern(n);
                        if blocked.contains(&sym) {
                            continue;
                        }
                        if let Some(f) = lookup(n) {
                            map.entry(sym).or_insert(FlatHit {
                                f,
                                owner: anc,
                                builtin: true,
                                frame_label: c_frame_label(anc, sym, '#'),
                            });
                        }
                    }
                }
            }
            map
        });
        Some(map.get(&name).copied())
    }

    /// `flat_value_hit`'s class-receiver twin: user `def self.x` rows over
    /// the builtin class-method table for `id` itself (no ancestry -- the
    /// walk the caller falls back to covers Class/Module).
    pub(super) fn flat_class_hit(
        &self,
        id: ClassId,
        name: Symbol,
    ) -> Option<(ValueMethodFn, Option<&'static str>)> {
        let entry = self.entries.get(&id.0)?;
        let map = entry.flat_class.get_or_init(|| {
            let mut map = crate::FMap::default();
            for (&sym, &f) in &entry.class_methods {
                map.entry(sym).or_insert((f, None));
            }
            if let Some(lookup) = crate::builtins::class_method_table(id) {
                for &n in crate::builtins::class_method_table_names(id) {
                    if let Some(f) = lookup(n) {
                        let sym = Symbol::intern(n);
                        map.entry(sym).or_insert((f, c_frame_label(id, sym, '.')));
                    }
                }
            }
            map
        });
        map.get(&name).copied()
    }

    /// Registers the no-`initialize` allocator backing `Class#allocate` --
    /// called once per user class from its generated `__register`, right after
    /// `register`. See `ClassEntry::allocator`.
    pub fn define_allocator(&mut self, id: ClassId, f: AllocatorFn) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.allocator = Some(f);
        }
    }

    /// Builds a fresh uninitialized instance of `id` via its registered
    /// allocator (`Class#allocate`), or `None` if the class registered none
    /// (a module/builtin). Does NOT run `initialize`.
    pub fn allocate_instance(&self, id: ClassId) -> Option<RubyValue> {
        let alloc = self.entries.get(&id.0).and_then(|e| e.allocator)?;
        Some(RubyValue::Object(alloc(id)))
    }

    /// Build an exception instance from a class NAME + message. Every built-in
    /// exception class registers its constructor `ConstructorFn` through
    /// `register_exceptions`, so the runtime constructs the object itself:
    /// allocate the struct and run `initialize(msg)` through the SAME trampoline
    /// `SomeError.new(msg)` uses. An unknown name is a zeo-rt bug (a `raise_error`
    /// site naming a class no exception defines). `Exception#initialize` only
    /// assigns `@message` and cannot signal, so a `Signal` here is a bug.
    ///
    /// `msg` is the WHOLE message, which matters for the `SystemCallError`
    /// family: their `initialize` composes one out of `strerror` plus its
    /// argument, and a raise site here has already composed the line CRuby
    /// prints ("No such file or directory @ rb_sysopen - /nope"). CRuby splits
    /// the same way -- `rb_syserr_fail_str` never runs `syserr_initialize`.
    pub fn construct_exception(&self, class_name: &str, msg: String) -> RubyValue {
        let id = self.by_name.get(class_name).copied();
        let ctor = id
            .and_then(|id| self.entries.get(&id))
            .and_then(|entry| entry.constructor);
        match (id, ctor) {
            (Some(id), Some(ctor)) => {
                let exc = ctor(
                    ClassId(id),
                    &[RubyValue::Str(crate::string_new(msg.clone()))],
                    None,
                )
                .expect("Exception#initialize can't signal");
                if class_name == "SystemCallError" || class_name.starts_with("Errno::") {
                    crate::builtins::exception::set_verbatim_message(&exc, msg);
                }
                exc
            }
            // No such class registered: a `raise_error` site naming a class no
            // exception defines (a zeo-rt bug), or a partial test registry.
            // Panic with the full message -- the same uncatchable fallback the
            // old registry-less `raise_error` used, so the real error still
            // surfaces rather than being masked by an "unknown class" note.
            _ => panic!("{class_name}: {msg}"),
        }
    }

    /// Records `name` as DEFINED DIRECTLY on `id` (not inherited) -- emitted by
    /// codegen for each of the class's own `def`s, beside `mark_private`. See
    /// `ClassEntry::own_methods`.
    pub fn mark_own(&mut self, id: ClassId, name: Symbol) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.own_methods.insert(name);
        }
    }

    /// [`mark_own`](Self::mark_own) for a `def self.x` -- see
    /// `ClassEntry::own_class_methods`.
    pub fn mark_own_class_method(&mut self, id: ClassId, name: Symbol) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.own_class_methods.insert(name);
        }
    }

    /// [`mark_own`](Self::mark_own), one call per class instead of one per
    /// method -- what codegen emits for a class's whole `def` list.
    pub fn mark_own_rows(&mut self, id: ClassId, names: &[&str]) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.own_methods
                .extend(names.iter().map(|n| Symbol::intern(n)));
        }
    }

    /// Records that `id` is the holder `module`'s `refine target` block
    /// minted -- see [`ClassEntry::refinement_of`].
    pub fn mark_refinement(&mut self, id: ClassId, module: ClassId, target: ClassId) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.refinement_of = Some((module, target));
        }
    }

    /// [`mark_own_class_method`](Self::mark_own_class_method)'s batch form.
    /// Record the modules `id`'s class body `extend`ed -- see
    /// [`ClassEntry::extends`]. Emitted only for a class that extends
    /// something, so the common program registers nothing.
    pub fn register_extends(&mut self, id: ClassId, mods: Vec<ClassId>) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.extends = mods;
        }
    }

    pub fn mark_own_class_method_rows(&mut self, id: ClassId, names: &[&str]) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.own_class_methods
                .extend(names.iter().map(|n| Symbol::intern(n)));
        }
    }

    /// [`define_value_method`](Self::define_value_method)'s batch form --
    /// the program's one `__VM_ROWS` static, applied after every `register`
    /// (each row's entry must exist; row order preserves last-wins).
    pub fn define_value_rows(&mut self, rows: &[(u32, u32, &str, ValueMethodFn)]) {
        for &(id, box_id, name, f) in rows {
            self.define_value_method(ClassId(id), box_id, Symbol::intern(name), f);
        }
    }

    /// The `__VM_FOREIGN` batch: which of the rows `define_value_rows` just
    /// installed came from an ancestor rather than this class -- see
    /// [`ClassEntry::foreign_value_names`].
    pub fn mark_foreign_value_rows(&mut self, rows: &[(u32, &str)]) {
        for &(id, name) in rows {
            if let Some(e) = self.entries.get_mut(&id) {
                e.foreign_value_names.insert(Symbol::intern(name));
            }
        }
    }

    /// [`define_class_method`](Self::define_class_method)'s batch form
    /// (`__CM_ROWS`).
    pub fn define_class_rows(&mut self, rows: &[(u32, &str, ValueMethodFn)]) {
        for &(id, name, f) in rows {
            self.define_class_method(ClassId(id), Symbol::intern(name), f);
        }
    }

    /// The `mark_private`/`mark_protected`/`mark_public` batch form
    /// (`__VIS_ROWS`; verb 0/1/2 respectively), plus the CLASS-method pair
    /// (`private_class_method`/`public_class_method`, verb 3/4). Rows apply IN
    /// ORDER: a later `public :m` promotion must clear an earlier
    /// private/protected stamp, and vice versa.
    pub fn mark_visibility_rows(&mut self, rows: &[(u32, &str, u8)]) {
        for &(id, name, verb) in rows {
            let sym = Symbol::intern(name);
            match verb {
                0 => self.mark_private(ClassId(id), sym),
                1 => self.mark_protected(ClassId(id), sym),
                3 => self.mark_class_method_private(ClassId(id), sym),
                4 => self.mark_class_method_public(ClassId(id), sym),
                _ => self.mark_public(ClassId(id), sym),
            }
        }
    }

    /// Records `name` as a PRIVATE class method of `id` -- what
    /// `private_class_method` marks. See `ClassEntry::private_class_methods`.
    pub fn mark_class_method_private(&mut self, id: ClassId, name: Symbol) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.private_class_methods.insert(name);
        }
    }

    /// `public_class_method`'s half: clears the mark above.
    pub fn mark_class_method_public(&mut self, id: ClassId, name: Symbol) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.private_class_methods.remove(&name);
        }
    }

    /// Registers a builtin-reopen method -- called from
    /// generated `main()` right after the builtin's own `register`, one call
    /// per `def` in a `class String ... end` reopen. `box_id` is the box the
    /// reopen was written in (0 for the root program; a box's
    /// overlay methods register under its id and are visible only from that
    /// box's code). See `ValueMethodFn`'s docs for the precedence contract.
    /// Records `name` as PRIVATE on `id` -- emitted by codegen right after
    /// the method's own registration, for each `def` whose resolved
    /// visibility is private. See `ClassEntry::private_methods`.
    pub fn mark_private(&mut self, id: ClassId, name: Symbol) {
        self.entries
            .get_mut(&id.0)
            .expect("class must be registered before marking its methods private")
            .private_methods
            .insert(name);
    }

    pub(super) fn is_private(&self, id: ClassId, name: Symbol) -> bool {
        self.entries
            .get(&id.0)
            .is_some_and(|e| e.private_methods.contains(&name))
    }

    pub(super) fn is_protected(&self, id: ClassId, name: Symbol) -> bool {
        self.entries
            .get(&id.0)
            .is_some_and(|e| e.protected_methods.contains(&name))
    }

    /// Records `name` as PROTECTED on `id` -- emitted by codegen beside
    /// `mark_private` for each `def` whose resolved visibility is protected.
    /// See `ClassEntry::protected_methods`.
    pub fn mark_protected(&mut self, id: ClassId, name: Symbol) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.protected_methods.insert(name);
        }
    }

    /// Restores `name` to PUBLIC on `id` -- clears any private/protected mark.
    /// Emitted for a `public :m` that promotes an inherited private/protected
    /// method (materialization first stamps it with the ancestor's visibility;
    /// this override wins). See `ClassEntry::visibility_overrides` in zeo.
    pub fn mark_public(&mut self, id: ClassId, name: Symbol) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.private_methods.remove(&name);
            e.protected_methods.remove(&name);
        }
    }

    /// The visibility `id` records for `name` IF it defines or materializes it
    /// (an own `def`, a builtin reopen, or an inherited method flattened in),
    /// else `None`. Answers only THIS class's table -- the MRO walk is the
    /// caller's (`instance_method_visibility`).
    pub(super) fn own_method_visibility(
        &self,
        id: ClassId,
        name: Symbol,
    ) -> Option<MethodVisibility> {
        let e = self.entries.get(&id.0)?;
        let defines = e.own_methods.contains(&name)
            || e.methods.contains_key(&name)
            || e.own_value_names.contains(&name);
        if !defines {
            return None;
        }
        Some(if e.protected_methods.contains(&name) {
            MethodVisibility::Protected
        } else if e.private_methods.contains(&name) {
            MethodVisibility::Private
        } else {
            MethodVisibility::Public
        })
    }

    pub fn define_value_method(
        &mut self,
        id: ClassId,
        box_id: u32,
        name: Symbol,
        f: ValueMethodFn,
    ) {
        let entry = self.entries.get_mut(&id.0).unwrap_or_else(|| {
            panic!(
                "class {} must be registered before defining `{}` on it",
                id.0,
                name.name()
            )
        });
        entry.value_methods.insert((box_id, name), f);
        entry.own_value_names.insert(name);
    }

    /// Replace an ALREADY-REGISTERED class's linearized ancestry -- the
    /// compile-time answer for a BOOTSTRAP class (the built-in exception tree)
    /// whose chain a reachable `C.prepend(M)` / `C.include(M)` changed. Those
    /// classes register once in `with_core`, so codegen has no `register` call
    /// of its own to carry the new chain, and re-registering would reinstall
    /// the native method set over the program's own deltas.
    pub fn set_ancestors(&mut self, id: ClassId, ancestors: Vec<ClassId>) {
        if let Some(e) = self.entries.get_mut(&id.0) {
            e.ancestors = ancestors;
        }
    }

    /// Records an `undef name` -- see `ClassEntry::undefined_methods`.
    pub fn mark_undefined(&mut self, id: ClassId, name: Symbol) {
        self.entries
            .get_mut(&id.0)
            .expect("class must be registered before undefining methods on it")
            .undefined_methods
            .insert(name);
    }

    /// Records an alias of an inherited BUILTIN method (see
    /// `ClassEntry::aliases`) -- emitted by codegen next to the class's
    /// registration. Pure data here; `validate_aliases` (run at program
    /// start, inside the fallible closure) is what raises `NameError` for a
    /// source that resolves nowhere.
    /// The NEW names `id`'s own builtin-alias rows define -- what reflection
    /// has to list, since an alias is a name indirection here rather than a
    /// copied method entry. Empty for an unregistered or aliasless id.
    pub(super) fn alias_names(&self, id: ClassId) -> Vec<Symbol> {
        self.entries
            .get(&id.0)
            .map(|e| e.aliases.keys().copied().collect())
            .unwrap_or_default()
    }

    pub fn register_alias(&mut self, id: ClassId, new: &str, old: &str) {
        self.entries
            .get_mut(&id.0)
            .expect("class must be registered before aliasing methods on it")
            .aliases
            .insert(Symbol::intern(new), Symbol::intern(old));
    }

    /// Whether `id`'s own body `undef`'d `name` -- the lookup TERMINATOR
    /// `respond_to?`'s ancestor walk consults.
    pub(super) fn is_undefined(&self, id: ClassId, name: Symbol) -> bool {
        self.entries
            .get(&id.0)
            .is_some_and(|e| e.undefined_methods.contains(&name))
    }

    /// [`Registry::register_alias`]'s singleton-side twin -- see
    /// `ClassEntry::class_aliases`.
    pub fn register_class_alias(&mut self, id: ClassId, new: &str, old: &str) {
        self.entries
            .get_mut(&id.0)
            .expect("class must be registered before aliasing class methods on it")
            .class_aliases
            .insert(Symbol::intern(new), Symbol::intern(old));
    }

    /// Registers one `def self.x` for dynamic dispatch -- see
    /// `ClassEntry::class_methods`' docs.
    pub fn define_class_method(&mut self, id: ClassId, name: Symbol, f: ValueMethodFn) {
        self.entries
            .get_mut(&id.0)
            .expect("class must be registered before defining class methods on it")
            .class_methods
            .insert(name, f);
    }

    /// Registers one `extend`ed-module method copy as a singleton-chain
    /// super target on the EXTENDING class -- see
    /// `ClassEntry::singleton_super_targets`.
    pub fn define_singleton_super_target(
        &mut self,
        id: ClassId,
        module: ClassId,
        name: Symbol,
        f: ValueMethodFn,
    ) {
        self.entries
            .get_mut(&id.0)
            .expect("class must be registered before defining class methods on it")
            .singleton_super_targets
            .insert((module.0, name), f);
    }

    /// The runtime-mutable path `define_method`/`define_singleton_method`
    /// actually go through: a real `HashMap` insert, no compile-time
    /// literal-name restriction (contrast zeo's `walk_scope`, which can
    /// only register a `define_method` as a scope if the name is a literal).
    pub fn define_method(&mut self, id: ClassId, name: Symbol, f: MethodFn) {
        self.entries
            .get_mut(&id.0)
            .expect("class must be registered before defining methods on it")
            .methods
            .insert(name, MethodImpl::from_fn(f));
    }

    /// `define_method` that ALSO records the row as this class's own
    /// `super` target -- the native exception method sets use this (every
    /// exception id carries the natives, so a `super` walk finds them at
    /// the first ancestor, behaviorally identical to CRuby finding them on
    /// `Exception`).
    pub fn define_method_own(&mut self, id: ClassId, name: Symbol, f: MethodFn) {
        let e = self
            .entries
            .get_mut(&id.0)
            .expect("class must be registered before defining methods on it");
        e.methods.insert(name, MethodImpl::from_fn(f));
        e.own_impls.insert(name, MethodImpl::from_fn(f));
    }

    /// Copies the flattened `methods` row for `name` into `own_impls` --
    /// emitted by codegen for each method a class defines DIRECTLY whose
    /// flattened winner is that own definition (everything except a
    /// prepend-shadowed own def), after all registration/delta rows landed.
    pub fn promote_own_impl(&mut self, id: ClassId, name: Symbol) {
        if let Some(e) = self.entries.get_mut(&id.0)
            && let Some(m) = e.methods.get(&name).cloned()
        {
            e.own_impls.insert(name, m);
        }
    }

    /// Registers a dynamic-self free function as an own `super` target --
    /// the bridge shape for an own def SHADOWED in its own class's
    /// flattened table (its body exists only as a `RubyValue`-self fn).
    pub fn define_super_target_value(&mut self, id: ClassId, name: Symbol, f: ValueMethodFn) {
        let e = self
            .entries
            .get_mut(&id.0)
            .expect("class must be registered before defining methods on it");
        e.own_impls.insert(
            name,
            MethodImpl::Dynamic(Arc::new(
                move |recv: &RObj, args: &[RubyValue], block: Option<RubyValue>| {
                    f(&RubyValue::Object(recv.clone()), args, block)
                },
            )),
        );
    }

    /// This class's OWN contribution to an MRO walk -- see
    /// `ClassEntry::own_impls`.
    pub(super) fn super_target(&self, id: ClassId, name: Symbol) -> Option<&MethodImpl> {
        self.entries.get(&id.0)?.own_impls.get(&name)
    }

    pub(super) fn lookup(&self, id: ClassId, name: Symbol) -> Option<&MethodImpl> {
        self.entries.get(&id.0)?.methods.get(&name)
    }

    /// `lookup` with CRuby's real resolution shape behind it: the flat
    /// materialized probe first (the hot path -- compile-time
    /// materialization flattens every reachable method onto each class, so
    /// this hit rate is ~100%), then a genuine ANCESTOR WALK over the
    /// registry entries as insurance for any row materialization missed.
    /// An `undef`'d name TERMINATES the walk at the class that undefined
    /// it (CRuby inserts a lookup-stopping "undefined" entry, it never
    /// deletes) -- checked per ancestor, so an ancestor's undef shadows a
    /// definition above it while the receiver's own materialized set stays
    /// authoritative below it. This is the dispatch shape `Ruby::Box`'s
    /// per-box overlays extend later (the walk gains a box dimension).
    pub(super) fn lookup_mro(&self, id: ClassId, name: Symbol) -> Option<&MethodImpl> {
        // A RUNTIME `undef_method` (`undef :m if <cond>`) tombstones the name in
        // the overlay, which this frozen table was built too early to know
        // about. Each position is checked there first so a tombstone terminates
        // the walk exactly as a compile-time `undef` does -- gated on
        // `is_live`, so the ordinary lock-free path is untouched.
        let overlay_live = crate::runtime_meta::is_live();
        let tombstoned =
            |cid: ClassId| overlay_live && crate::runtime_meta::overlay_is_undefined(cid, name);
        if tombstoned(id) {
            return None;
        }
        // A position that has no object-channel row but DOES have a VALUE one
        // (a builtin REOPEN, and every method of a namespace-slot builtin whose
        // body is Ruby -- `WeakRef`) ends this walk: answering from further up
        // would step over the nearer definition. The caller's own per-ancestor
        // walk probes both channels and resolves it. Gated on the entry having
        // any value rows at all, which no ordinary user class does.
        let shadowed =
            |e: &ClassEntry| !e.own_value_names.is_empty() && e.own_value_names.contains(&name);
        if let Some(e) = self.entries.get(&id.0) {
            if let Some(m) = e.methods.get(&name) {
                // The flattened row may have come from an ANCESTOR that a
                // runtime `undef_method` has since retired -- `module M; def
                // doomed; end; end` mixed into a class, then `M.undef_method
                // :doomed`. Materialization copied the body onto every
                // includer, so the tombstone sits somewhere this probe never
                // looks. Only when the overlay is live, and only up to the
                // position that really defines the name: a nearer own
                // definition still wins, as it does in ruby.
                if overlay_live && self.retired_before_owner(id, name) {
                    return None;
                }
                return Some(m);
            }
            if e.undefined_methods.contains(&name) || shadowed(e) {
                return None;
            }
        }
        for &anc in self.ancestors_of(id).iter().skip(1) {
            if tombstoned(anc) {
                return None;
            }
            let Some(e) = self.entries.get(&anc.0) else {
                continue;
            };
            if let Some(m) = e.methods.get(&name) {
                return Some(m);
            }
            if e.undefined_methods.contains(&name) || shadowed(e) {
                return None;
            }
        }
        None
    }

    /// Whether `name` is defined DIRECTLY on `id` (a `def` here, or a builtin
    /// reopen), as opposed to materialized in from an ancestor. This is the
    /// signal `Method#owner` needs: materialization copies an inherited method
    /// onto every descendant's `methods` table, so `lookup` can't tell where it
    /// originated, but `own_methods`/`value_methods` record only local defs.
    /// Whether an `undef_method` tombstone sits between `id` and the position
    /// that actually defines `name` -- see the flattened-row check in
    /// [`Registry::lookup_mro`].
    pub(super) fn retired_before_owner(&self, id: ClassId, name: Symbol) -> bool {
        for &anc in self.ancestors_of(id) {
            if crate::runtime_meta::overlay_is_undefined(anc, name) {
                return true;
            }
            if self.defines_own(anc, name) {
                return false;
            }
        }
        false
    }

    pub(super) fn defines_own(&self, id: ClassId, name: Symbol) -> bool {
        let Some(e) = self.entries.get(&id.0) else {
            return false;
        };
        e.own_methods.contains(&name) || e.own_value_names.contains(&name)
    }

    /// This class's registered instance-method names, each tagged private/not,
    /// excluding `undef`'d names -- the registry half of `instance_methods`/
    /// `methods` reflection. `own_only` narrows the user-method set to those
    /// DEFINED DIRECTLY on the class (for `instance_methods(false)`); builtin
    /// reopens (`value_methods`) are always own. When `own_only` is set but the
    /// class recorded no own-set (e.g. a builtin with no reopens), nothing is
    /// dropped only because there is nothing to drop.
    pub(super) fn own_instance_method_names(
        &self,
        id: ClassId,
        own_only: bool,
    ) -> Vec<(Symbol, MethodVisibility)> {
        let Some(e) = self.entries.get(&id.0) else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let consider = |name: Symbol,
                        seen: &mut HashSet<Symbol>,
                        out: &mut Vec<(Symbol, MethodVisibility)>| {
            if e.undefined_methods.contains(&name) || !seen.insert(name) {
                return;
            }
            let vis = if e.protected_methods.contains(&name) {
                MethodVisibility::Protected
            } else if e.private_methods.contains(&name) {
                MethodVisibility::Private
            } else {
                MethodVisibility::Public
            };
            out.push((name, vis));
        };
        // `own_methods` is the authoritative "defined directly here" set --
        // and the ONLY reliable source for a MODULE, whose instance methods
        // live on its includers rather than its own `methods` map.
        for &name in &e.own_methods {
            consider(name, &mut seen, &mut out);
        }
        // Builtin-reopen methods are always own to the reopened class.
        for (_, name) in e.value_methods.keys() {
            consider(*name, &mut seen, &mut out);
        }
        if !own_only {
            // The materialized set adds the inherited methods flattened onto a
            // concrete class at compile time.
            for name in e.methods.keys().copied() {
                consider(name, &mut seen, &mut out);
            }
        }
        out
    }

    /// This class's OWN `def self.x` names -- the registry half of
    /// `SomeClass.singleton_methods`/`.methods`. `own_only` narrows to the
    /// names written HERE (`singleton_methods(false)`); the wide form is the
    /// materialized map, which already carries every inherited class method.
    pub(super) fn own_class_method_names(&self, id: ClassId, own_only: bool) -> Vec<Symbol> {
        let Some(e) = self.entries.get(&id.0) else {
            return Vec::new();
        };
        match own_only {
            true => e.own_class_methods.iter().copied().collect(),
            false => e.class_methods.keys().copied().collect(),
        }
    }

    /// The `(caller's box, name)` probe with the root fallback -- CRuby's
    /// def->box resolution rule: a box's own patch wins inside the box,
    /// root patches are visible everywhere, and nothing else is.
    pub(super) fn lookup_value_method(
        &self,
        id: ClassId,
        box_id: u32,
        name: Symbol,
    ) -> Option<ValueMethodFn> {
        let entry = self.entries.get(&id.0)?;
        if box_id != 0
            && let Some(f) = entry.value_methods.get(&(box_id, name))
        {
            return Some(*f);
        }
        entry.value_methods.get(&(0, name)).copied()
    }

    pub(super) fn ancestors_of(&self, id: ClassId) -> &[ClassId] {
        self.entries.get(&id.0).map_or(&[], |e| &e.ancestors)
    }
}

/// Every registered class that has `id` in its ancestry, `id` itself excluded
/// -- the descendants a change to `id` can be seen through. One scan of the
/// registry, called only from `runtime_meta::patch_class` on a real runtime
/// definition, never from a loop.
///

/// The class registry is installed exactly once, from generated `main()`,
/// before any `Thread`/`Ractor` spawns anything -- a `OnceLock`
/// (not a `thread_local!`, unlike before the Send+Sync migration) gives
/// lock-free reads forever after that single write, and is itself the
/// correct semantic choice regardless of concurrency: classes/methods are
/// genuinely process-wide-shared in real Ruby, not per-thread state.
pub(super) static REGISTRY: OnceLock<ClassRegistry> = OnceLock::new();

pub(super) fn registry() -> &'static ClassRegistry {
    REGISTRY
        .get()
        .expect("class registry not installed -- install_class_registry must run first")
}

/// Whether an instance method `name` is registered directly on class `id` (its
/// flat, materialized `methods` table) -- the value-subclass constructor uses
/// this to decide whether to run a user `initialize` or seed the payload from
/// the args directly.
pub fn has_instance_method(id: ClassId, name: Symbol) -> bool {
    registry().lookup(id, name).is_some()
}

/// Class ids carrying a box-0 `to_s`/`inspect` builtin reopen, precomputed at
/// install: `display_with`/`inspect_with` probe the registry per RENDERED
/// VALUE to honor `class Integer; def to_s`-style overrides, and almost every
/// program has none -- this set makes that probe a lock-free contains check.
static DISPLAY_REOPENS: OnceLock<crate::FSet<u32>> = OnceLock::new();

/// Whether `id` might carry a display-affecting reopen. `true` before the
/// registry is installed (unit tests run registry-less), so the probe is
/// never skipped when it could matter.
pub(crate) fn has_display_reopen(id: ClassId) -> bool {
    match DISPLAY_REOPENS.get() {
        Some(set) => set.contains(&id.0),
        None => true,
    }
}

/// Called once from generated `main()`, after every class's `__register` has
/// populated the registry passed in.
pub fn install_class_registry(registry: ClassRegistry) {
    let to_s = crate::symbol::wk::to_s();
    let inspect = crate::symbol::wk::inspect();
    let mut reopens = crate::FSet::default();
    for (id, entry) in &registry.entries {
        if entry
            .value_methods
            .keys()
            .any(|&(b, s)| b == 0 && (s == to_s || s == inspect))
        {
            reopens.insert(*id);
        }
    }
    let _ = DISPLAY_REOPENS.set(reopens);
    REGISTRY
        .set(registry)
        .unwrap_or_else(|_| panic!("class registry installed twice"));
}
