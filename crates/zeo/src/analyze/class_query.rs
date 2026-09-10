//! Questions about the class a method body is being emitted FOR.
//!
//! Two questions: where an ivar sits in the receiver's slot layout, and
//! whether a bare name resolves to a real method or falls through to a
//! Kernel free function. The Cranelift backend emits per-class bodies and
//! asks them directly; no record-and-replay of shared bodies is needed.

use crate::compiler::{ClassId, Compiler, OBJECT_CLASS};

/// Whether `cid` has its own definition of `name`, with `Object` answering NO
/// however its chain reads.
///
/// A Kernel function's own ruby-level definition materializes on `Object`
/// (`Kernel#pp` is pp.rb's), so a hit there proves nothing about whether a
/// bare name is shadowed.
pub(crate) fn shadows_kernel(compiler: &Compiler, cid: Option<ClassId>, name: &str) -> bool {
    cid.is_some_and(|cid| cid != OBJECT_CLASS && compiler.method_in_chain(cid, name).is_some())
}

/// Whether the program gives `name` a definition every receiver-less call
/// sees -- a `def` on `Object`, on `Kernel`, or on a module included into
/// `Object`.
///
/// [`shadows_kernel`] answers the ENCLOSING class's question and says no for
/// `Object` on purpose. This asks the universal one, which is what a fold
/// that replaces a Kernel primitive with a direct call has to consult:
/// `module Kernel; def puts; end; end` made every folded `puts` in the
/// program keep printing the builtin's output.
pub(crate) fn overrides_kernel_universal(compiler: &Compiler, name: &str) -> bool {
    compiler.class(OBJECT_CLASS).ancestors.iter().any(|&anc| {
        compiler
            .class(anc)
            .own_methods
            .iter()
            .any(|&s| compiler.scope(s).name == name)
    })
}

/// The ancestor whose SINGLETON-chain slot holds `defining_class`: the class
/// itself when a `def self.x` defines the method, or the class that `extend`s
/// (or singleton-prepends) it when a module does. `None` when the receiver's
/// ancestry holds no such slot.
///
/// This is what a runtime class-method `super` must resume from. The chain
/// puts an extended module directly after the class extending it (see
/// [`extended_singleton_super`]), so resuming after that class is exactly
/// right: the class's own `def self.x` sits BEFORE the module and is
/// correctly skipped.
pub(crate) fn singleton_chain_host(
    compiler: &Compiler,
    receiver_class: ClassId,
    defining_class: Option<ClassId>,
) -> Option<ClassId> {
    let defining_class = defining_class?;
    compiler
        .class(receiver_class)
        .ancestors
        .iter()
        .enumerate()
        .find(|&(i, &anc)| {
            let info = compiler.class(anc);
            if i > 0 && info.is_module {
                return false;
            }
            anc == defining_class
                || info.extends.contains(&defining_class)
                || info.class_method_prepends.contains(&defining_class)
        })
        .map(|(_, &anc)| anc)
}

/// `super` resolution for a method that reached the receiver as a CLASS
/// method via `extend M`: walks the receiver's SINGLETON-class chain as the
/// compiler knows it -- for each non-module ancestor (`include`d modules
/// never join a singleton chain), the ancestor's own `def self.x` pool and
/// then its `extend`ed modules' instance-method pools, most recently
/// extended first. Returns the first `mname` definition STRICTLY AFTER
/// `defining_class`'s own entry in that chain, `None` when the walk runs
/// dry (the caller then defers to the runtime walk, which owns builtin
/// defaults and runtime-defined methods).
pub(crate) fn extended_singleton_super(
    compiler: &Compiler,
    receiver_class: ClassId,
    defining_class: ClassId,
    mname: &str,
) -> Option<(ClassId, crate::compiler::ScopeId, bool)> {
    // `(class, instance_pool)`: a chain entry resolves `mname` against its
    // instance methods (an extended module) or its `def self.x` pool (a
    // class standing in for its own metaclass).
    let mut chain: Vec<(ClassId, bool)> = Vec::new();
    for (i, &anc) in compiler.class(receiver_class).ancestors.iter().enumerate() {
        let info = compiler.class(anc);
        // The receiver itself heads the chain even when it IS a module
        // (`module Target; extend Props; end`); mixed-in modules deeper in
        // the MRO contribute nothing to the singleton chain.
        if i > 0 && info.is_module {
            continue;
        }
        // Singleton-PREPENDED modules sit BEFORE this ancestor's own class
        // methods (they override `def self.x`, `super` reaching the original),
        // most recently prepended first -- the class-method mirror of `prepend`
        // on the instance chain.
        for &m in info.class_method_prepends.iter().rev() {
            chain.push((m, true));
        }
        chain.push((anc, false));
        for &m in info.extends.iter().rev() {
            chain.push((m, true));
        }
    }
    // The defining entry: the extended MODULE (`super` written in it,
    // instance pool) or the CLASS itself (`def self.x`'s own slot) --
    // module and class ids never collide, so the id alone identifies it.
    let dpos = chain.iter().position(|&(c, _)| c == defining_class)?;
    chain[dpos + 1..].iter().find_map(|&(anc, instance_pool)| {
        let info = compiler.class(anc);
        let pool = if instance_pool {
            &info.own_methods
        } else {
            &info.own_class_methods
        };
        pool.iter()
            .find(|&&s| compiler.scope(s).name == mname)
            .map(|&sid| {
                // A class's OWN `def self.x` that a singleton PREPEND shadows is
                // not in the live class-methods row (the prepend won), so
                // `super` must reach it through the super-TARGET table -- the
                // `module_instance` side of `call_singleton_super_target`, which
                // `mro::materialize_class_methods` populated with the shadowed
                // own copy. Report it there instead of the (occupied) class row.
                let shadowed_by_prepend = !instance_pool
                    && info.class_method_prepends.iter().any(|&pm| {
                        compiler
                            .class(pm)
                            .own_methods
                            .iter()
                            .any(|&s| compiler.scope(s).name == mname)
                    });
                (anc, sid, instance_pool || shadowed_by_prepend)
            })
    })
}

/// The `IvarCell` slot index of `@name` on `class`: hidden ivars (Struct/Data
/// members) FIRST, then declared ivars after them.
///
/// Members first is what lets a subclass of a compiled struct share its
/// ancestor's compiled bodies. The subclass inherits the member list, so
/// laying the members out after the declared ivars moved them the moment the
/// subclass declared one of its own -- `class Sub < Struct.new(:example)`
/// with an `@p` put `@p` at the slot Struct's `initialize` writes.
pub(crate) fn slot_of(compiler: &Compiler, class: ClassId, name: &str) -> Option<usize> {
    let info = compiler.class(class);
    info.hidden_ivars
        .iter()
        .position(|iv| iv == name)
        .or_else(|| {
            info.ivars
                .iter()
                .position(|iv| iv == name)
                .map(|i| info.hidden_ivars.len() + i)
        })
}

/// `K.method(:name)` where every own `def self.name` sits LATER in the
/// document than the capture at `at`. CRuby resolves at capture time, so
/// the Method binds the INHERITED entry -- a by-name capture would find
/// the later override and, for rspec-support's `NEW_MUTEX_METHOD =
/// Mutex.method(:new)` / `def self.new = NEW_MUTEX_METHOD.call` pair,
/// recurse forever.
///
/// SPANS, not `doc_order`: a plain `def` is registration rather than an
/// executable site statement, so the position index never records it --
/// but source offsets order a SAME-FILE body just as well, and the
/// capture-before-own-def shape is a same-file one (a cross-file reopen
/// stays on the normal capture).
pub(crate) fn class_method_defined_only_later(
    compiler: &Compiler,
    target: ClassId,
    sym: &str,
    at: crate::hir::NodeId,
) -> bool {
    let Some(at) = compiler.hir.span(at) else {
        return false;
    };
    let mut any = false;
    for (n, is_cm, _seq, sid) in &compiler.class(target).method_history {
        if *is_cm && n == sym {
            let Some(def_node) = compiler.scope(*sid).def_node else {
                return false;
            };
            let Some(def) = compiler.hir.span(def_node) else {
                return false;
            };
            if def.file != at.file || def.start < at.start {
                return false;
            }
            any = true;
        }
    }
    any
}

/// Whether evaluating `id`'s subtree can ASSIGN the local `name`.
///
/// A borrowed receiver stays borrowed across the argument evaluation, and
/// `a << (a = [9]; 2)` is perfectly good Ruby -- the push lands on the
/// array the receiver named BEFORE the argument ran. A write from inside a
/// nested BLOCK is not a case this has to find: a closure that could
/// reassign the local would have forced cell storage, which no backend
/// borrows through.
pub(crate) fn assigns_local(compiler: &Compiler, id: crate::hir::NodeId, name: &str) -> bool {
    use crate::hir::HirNode;
    let node = &compiler.hir[id];
    let own = match node {
        HirNode::LocalWrite(n, _) => n == name,
        HirNode::MultiWrite { targets, .. } => {
            let mut names = Vec::new();
            targets.collect_local_names(&mut names);
            names.iter().any(|n| n == name)
        }
        _ => false,
    };
    if own {
        return true;
    }
    let mut found = false;
    node.for_each_child(&mut |child| {
        found = found || assigns_local(compiler, child, name);
    });
    found
}
