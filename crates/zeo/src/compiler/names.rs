//! Name interning ([`Names`]) and [`Compiler`]'s name-resolution and
//! class-identity queries (cref chains, fully-qualified names).

use super::*;

/// The reserved name a `class << self` body's own class registers under.
///
/// It is the singleton class of the enclosing class, as a real compile-time
/// class: `zeo_rt::register_singleton_surrogate` makes `Foo.singleton_class`
/// answer this class at run time, and constants, `def`s and residual runtime
/// statements from the singleton body all belong to it.
///
/// No Ruby constant can collide with it -- a constant must start with an
/// uppercase letter -- which is what makes it safe to mint under a name the
/// program can also write.
pub const SINGLETON_SURROGATE: &str = "#<Class:self>";

/// An interned method name. Method tables are the one place in the compiler
/// with a name per (class, name) pair rather than per definition, so this is
/// where string comparison and per-entry `String` allocation actually cost
/// something -- the `FSet<String>::insert` that was 47% of the Rails-scale
/// profile was exactly this.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct NameId(u32);

#[derive(Default)]
pub struct Names {
    ids: FMap<Box<str>, NameId>,
    list: Vec<Box<str>>,
}

impl Names {
    pub fn intern(&mut self, name: &str) -> NameId {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = NameId(self.list.len() as u32);
        let boxed: Box<str> = name.into();
        self.list.push(boxed.clone());
        self.ids.insert(boxed, id);
        id
    }

    /// The id `name` already has, or `None` -- a lookup, which must not mint a
    /// new id for a name nothing ever defined.
    pub fn get(&self, name: &str) -> Option<NameId> {
        self.ids.get(name).copied()
    }

    pub fn str(&self, id: NameId) -> &str {
        &self.list[id.0 as usize]
    }
}

/// Whether `ZEO_DEBUG=verify-class-index` is set: every `class_in_scope`
/// answer is then shadow-compared against the original linear scan -- the
/// drift detector for the index's settle-before-lookup registration
/// invariant.
fn verify_class_index() -> bool {
    crate::debug_flags::debug(crate::debug_flags::DebugFlag::VerifyClassIndex)
}

impl Compiler {
    /// THE name-resolution primitive -- every "which class does
    /// this name/path mean HERE" question goes through this one function,
    /// keyed by the full resolution context real Ruby uses: the lexical
    /// cref chain (innermost scope LAST -- `cref_of`'s order), and the box
    /// the referencing code is defined in (`0` at the top level).
    ///
    /// `path` may be a multi-segment constant path:
    /// `"Store::Errors::NotFound"` resolves its FIRST segment through the
    /// full unqualified rule below, then descends the remaining segments as
    /// direct namespace children only (no lexical/bootstrap fallback past
    /// the first segment -- real Ruby's own `::` rule). A leading `::`
    /// (`"::Foo"`) anchors the first segment at the top level, skipping the
    /// cref chain.
    ///
    /// Unqualified rule, mirroring CRuby: the lexical chain
    /// innermost-outward, then the box's own top level, then the BOOTSTRAP
    /// set (builtins + the built-in exceptions -- the "defined before any
    /// user program runs" classes every box sees; a box's own definition of
    /// the same name shadows it, exactly like CRuby's per-box constant
    /// overlay). First-registered wins within one scope, same as the old
    /// flat `class_by_name` (reopening semantics attach to that first
    /// registration rather than adding duplicates). One
    /// documented approximation: the cref head's ANCESTORS are not searched
    /// (real Ruby checks them between the lexical chain and the top level
    /// -- a class nested inside a SUPERCLASS referenced by bare name from a
    /// subclass misses here, loudly, rather than resolving wrong).
    pub fn resolve_class(&self, path: &str, cref: &[ClassId], box_id: u32) -> Option<ClassId> {
        let parsed = crate::constpath::ConstPath::parse(path);
        let anchored = parsed.is_top_anchored();
        let mut segments = parsed.segments();
        let first = segments.next()?;
        let mut cur = self.resolve_unqualified(first, if anchored { &[] } else { cref }, box_id)?;
        for seg in segments {
            // Descend within the resolved parent's OWN box (the parent may
            // itself have resolved through the bootstrap fallback into box
            // 0 even when `box_id` differs).
            cur = self.class_in_scope(Some(cur), seg, self.class(cur).box_id)?;
        }
        // A per-box builtin-reopen OVERLAY is a patch container,
        // never a distinct class: as a resolved NAME it collapses to the
        // root builtin (`box::String == String` stays true; instances keep
        // the root's identity). Reopen-merge detection deliberately uses
        // the raw `class_in_scope` instead.
        Some(self.class(cur).builtin_overlay.unwrap_or(cur))
    }

    /// `resolve_class`'s single-segment core -- see its docs for the rule.
    fn resolve_unqualified(&self, name: &str, cref: &[ClassId], box_id: u32) -> Option<ClassId> {
        for &scope in cref.iter().rev() {
            if let Some(cid) = self.class_in_scope(Some(scope), name, box_id) {
                return Some(cid);
            }
        }
        // After the lexical nesting, Ruby consults the ANCESTRY of the
        // innermost lexical module -- a constant nested in an INCLUDED module
        // resolves unqualified. `ancestors` is linearized by mro before
        // codegen (empty during analyze, so this is a no-op there).
        if let Some(&innermost) = cref.last() {
            for &anc in &self.class(innermost).ancestors {
                if anc != innermost
                    && let Some(cid) = self.class_in_scope(Some(anc), name, box_id)
                {
                    return Some(cid);
                }
            }
        }
        if let Some(cid) = self
            .class_in_scope(None, name, box_id)
            .filter(|&c| self.feature_active(c))
        {
            return Some(cid);
        }
        if box_id != 0 {
            // The index's (0, None) row IS the first-registered top-level
            // name; builtins and bootstrap classes register before any user
            // class, so when a builtin/bootstrap of this name exists it is
            // that first entry -- and when the entry is a user class, no
            // builtin of the name exists and the old scan also missed.
            if let Some(cid) = self
                .class_in_scope(None, name, 0)
                .filter(|&c| {
                    let ci = self.class(c);
                    // The implicit `Object` root predates the builtin
                    // placeholders and carries neither flag, so it is
                    // checked by id -- without which a box's own `class
                    // Object` (which is what a top-level `def` in a box
                    // means) minted a SECOND class called `Object`
                    // instead of the box's overlay of the real one.
                    ci.is_builtin || ci.is_bootstrap || c == OBJECT_CLASS
                })
                .filter(|&c| self.feature_active(c))
            {
                return Some(cid);
            }
        }
        // A top-level constant alias for a `Thread::`-nested builtin
        // (`::Queue = Thread::Queue`, ...) -- consulted LAST so a user's own
        // top-level `Queue`/`Mutex`/etc. shadows it, as in real Ruby.
        zeo_abi::TOP_LEVEL_ALIASES
            .iter()
            .find(|(alias, _)| *alias == name)
            .map(|&(_, cid)| cid)
            .filter(|&c| self.feature_active(c))
    }

    /// First class/module named `name` defined directly inside
    /// `lexical_parent` (or at the top level for `None`) in `box_id`.
    /// `pub(crate)` because `analyze::register_class`'s
    /// reopening-detection must be SCOPE-EXACT (a nested `Store::Item` must
    /// never be mistaken for a top-level `Item`, or vice versa), which the
    /// lexical-fallback walk `resolve_class` does would get wrong.
    pub(crate) fn class_in_scope(
        &self,
        lexical_parent: Option<ClassId>,
        name: &str,
        box_id: u32,
    ) -> Option<ClassId> {
        let mut index = self.class_index.borrow_mut();
        let upto = self.indexed_upto.get();
        if upto < self.classes.len() {
            // First-registered wins within one scope (`or_insert`), exactly
            // the old scan's `position` semantics.
            for (i, c) in self.classes.iter().enumerate().skip(upto) {
                index
                    .entry((c.box_id, c.lexical_parent))
                    .or_default()
                    .entry(c.name.clone())
                    .or_insert(ClassId(i as u32));
            }
            self.indexed_upto.set(self.classes.len());
        }
        let hit = index
            .get(&(box_id, lexical_parent))
            .and_then(|m| m.get(name))
            .copied();
        if verify_class_index() {
            let scan = self
                .classes
                .iter()
                .position(|c| {
                    c.name == name && c.box_id == box_id && c.lexical_parent == lexical_parent
                })
                .map(|i| ClassId(i as u32));
            assert_eq!(
                hit, scan,
                "class index diverged for {name:?} (box {box_id}, parent {lexical_parent:?})"
            );
        }
        hit
    }

    /// Whether ANY `class`/`module` in `box_id` is spelled `name` or ends in
    /// `::name` -- i.e. whether the name is class-shaped SOMEWHERE, whatever
    /// scope it was written under.
    ///
    /// Both callers ask this to stay honest about a definition they cannot see
    /// yet: `analyze`'s class-body splice folds before the body's own
    /// statements register, and `guard_fold` may be looking at a name written
    /// under a scope the reference spells differently (`Psych::Visitors` from
    /// inside `module Psych`). Neither may answer "not defined" then.
    ///
    /// One implementation because it was two, keyed and suffixed identically,
    /// in files that do not otherwise share code.
    pub fn class_shaped_anywhere(&self, box_id: u32, name: &str) -> bool {
        let suffix = format!("::{name}");
        self.shell_kinds
            .keys()
            .any(|(bx, k)| *bx == box_id && (k == name || k.ends_with(&suffix)))
    }

    /// The `#<Class:self>` surrogate standing in for `owner`'s `class << self`
    /// body, if it has one.
    ///
    /// Both callers used to scan every registered class for it, from inside
    /// per-class loops -- the same scan written twice, quadratic in the class
    /// count. `class_in_scope` answers it with the same first-registered-wins
    /// rule, and the surrogate shares its owner's box because a `class <<
    /// self` body is lexically inside the owner.
    pub(crate) fn singleton_surrogate_of(&self, owner: ClassId) -> Option<ClassId> {
        self.class_in_scope(Some(owner), SINGLETON_SURROGATE, self.class(owner).box_id)
    }

    /// Whether `cid` is a singleton-class SURROGATE -- the module a
    /// constant-bearing `class << self` body registers under the reserved
    /// name (see `lower::defs`). Display name and reflection identity come
    /// from `fq_name`'s `#<Class:M>` special case.
    pub fn is_singleton_surrogate(&self, cid: ClassId) -> bool {
        self.class(cid).name == SINGLETON_SURROGATE
    }

    /// Precomputes `cref_of`/`fq_name` for every class -- see
    /// [`Compiler::frozen_crefs`]. Called once, at `mro::materialize`'s
    /// head: registration (the only phase that adds classes or writes
    /// their identity fields) is over by then.
    pub fn freeze_identity_caches(&mut self) {
        let crefs = (0..self.classes.len() as u32)
            .map(|i| self.cref_of(Some(ClassId(i))))
            .collect();
        let names = (0..self.classes.len() as u32)
            .map(|i| self.fq_name(ClassId(i)))
            .collect();
        self.frozen_crefs = Some(crefs);
        self.frozen_fq_names = Some(names);
        let defs = self
            .classes
            .iter()
            .map(|ci| {
                ci.class_body_stmts
                    .iter()
                    .filter_map(|&n| match &self.hir[n] {
                        crate::hir::HirNode::ConstWrite {
                            scope: None, name, ..
                        } => Some(name.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        self.direct_const_defs = Some(defs);
    }

    /// `cref_of` as a borrowed slice, alloc-free -- valid only after
    /// `freeze_identity_caches` (any post-registration caller).
    pub fn cref_of_ref(&self, cid: ClassId) -> &[ClassId] {
        self.frozen_crefs
            .as_ref()
            .and_then(|c| c.get(cid.0 as usize))
            .expect("cref_of_ref before freeze_identity_caches")
    }

    /// The lexical cref chain enclosing (and including) `defining`,
    /// OUTERMOST FIRST -- exactly the `cref` argument `resolve_class`
    /// takes. Walks `cref_parent` links, which are `lexical_parent` except
    /// where a qualified definition parts the two (see
    /// `ClassInfo::cref_parent`).
    pub fn cref_of(&self, defining: Option<ClassId>) -> Vec<ClassId> {
        if let (Some(cache), Some(cid)) = (&self.frozen_crefs, defining)
            && let Some(chain) = cache.get(cid.0 as usize)
        {
            return chain.clone();
        }
        let mut chain = Vec::new();
        let mut cur = defining;
        while let Some(cid) = cur {
            chain.push(cid);
            // Nesting deeper than this is a `lexical_parent` cycle, not real
            // source. Bounded rather than tracked with a set: this runs on
            // every constant lookup, and a real chain is a handful of links.
            if chain.len() > MAX_NESTING {
                break;
            }
            cur = self.class(cid).cref_parent;
        }
        chain.reverse();
        chain
    }

    /// The fully-qualified display name (`"Store::Errors::NotFound"`) --
    /// joins the `lexical_parent` chain regardless of `qualified_def`
    /// (naming is a namespace property, cref cutting is not). Used for
    /// error messages wherever real Ruby prints the qualified path.
    pub fn fq_name(&self, cid: ClassId) -> String {
        if let Some(cache) = &self.frozen_fq_names
            && let Some(name) = cache.get(cid.0 as usize)
        {
            return name.clone();
        }
        let mut segments = Vec::new();
        let mut cur = Some(cid);
        while let Some(c) = cur {
            // A singleton-class surrogate (a `class << self` body -- see
            // `lower::defs`) displays as CRuby's `#<Class:M>`, not a `M::...`
            // path: it is not reachable as a constant at all. It ENDS the
            // walk, because that rendering already carries its own parent --
            // and a class nested inside the body has to take the same prefix
            // or `Module#constants` cannot match its qualified name against
            // the surrogate's (which is what hid such a class from
            // `M.singleton_class.constants`).
            if self.class(c).name == SINGLETON_SURROGATE
                && let Some(p) = self.class(c).lexical_parent
            {
                segments.push(format!("#<Class:{}>", self.fq_name(p)));
                break;
            }
            segments.push(self.class(c).name.clone());
            if segments.len() > MAX_NESTING {
                break;
            }
            cur = self.class(c).lexical_parent;
        }
        segments.reverse();
        segments.join("::")
    }

    /// The LEAF segment of `cid`'s name -- what CRuby puts in a class-body
    /// backtrace frame (`<module:B>`, never `<module:A::B>`), regardless of how
    /// deeply the class is nested or whether it was defined compact
    /// (`module A::B`) or nested. Oracle-verified against ruby 4.0.6.
    pub fn leaf_name(&self, cid: ClassId) -> &str {
        crate::constpath::ConstPath::parse(&self.class(cid).name).base()
    }

    /// Whether the program assigns the constant `path` NAMES -- as opposed to
    /// some constant that merely shares its last segment.
    ///
    /// A bare `Template` can be assigned in any lexical scope, so its leaf is
    /// the only question worth asking. `::Tilt::Template` is a different
    /// question: temple writes `class Tilt < ::Tilt::Template` and, with tilt
    /// absent from the require graph, the leaf test found some unrelated
    /// `Template` and called the superclass known -- so the definition was
    /// registered rather than deferred, and failed later as an unknown
    /// superclass instead of raising the `NameError` CRuby raises there.
    pub fn assigns_const_path(&self, path: &str) -> bool {
        let path = crate::constpath::ConstPath::parse(path);
        let key = if path.is_bare() {
            path.base()
        } else {
            path.unanchored()
        };
        self.assigned_const_names.contains(key)
    }
}
