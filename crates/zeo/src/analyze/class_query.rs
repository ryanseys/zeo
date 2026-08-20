//! Every question an emitted method body asks about the class it is being
//! emitted FOR -- and the answers, recorded so another class can replay them.
//!
//! One `def` inherited by 152 classes is one body. Whether those classes can
//! share a single emitted function is decided by what the body actually asks
//! about its receiver: where an ivar sits, whether a bare name resolves to a
//! real method or to a Kernel free function, which channel a `super` takes.
//! Everything else a body reads -- constants, `@@cvars`, `local_types`, frame
//! labels, `super`'s resume id -- hangs off the DEFINITION's `defining_class`
//! and is therefore identical for every class in the group by construction.
//!
//! The alternative was a hand-written list of those questions. There are 28
//! reads of the receiver class in the emitter, and a list of them is exactly
//! the kind of thing that rots: a read added later and not added to the list is
//! a WRONG PROGRAM (two classes share a body that needed to differ), not a
//! missed optimization. So the emitter records the questions it really asked,
//! and other classes replay them.
//!
//! [`ClassQuery::answer`] is the only evaluator. Recording and replay both go
//! through it, so a trace taken on one class is a decision procedure for
//! another rather than a description of the first.

use crate::compiler::{ClassId, Compiler, OBJECT_CLASS};

/// One question about the receiver class.
///
/// A variant carries whatever the question needs BESIDES the class -- an ivar
/// name, a method name, the definition's own class. Those parts are constant
/// across a sharing group; the class is the variable.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum ClassQuery {
    /// Which slot of the receiver's `IvarCell` holds `@name` ([`slot_of`]).
    /// The emitted code carries the INDEX, so two classes that place the name
    /// differently -- or one that never declares it -- must not share.
    IvarSlot(String),
    /// Does the receiver's ancestry hold a real method called `name`? Asked by
    /// the `eval` interception.
    InChain(String),
    /// [`ClassQuery::InChain`], but `Object` answers NO however its chain
    /// reads: a Kernel function's own ruby-level definition materializes there
    /// (`Kernel#pp` is pp.rb's), so a hit on `Object` proves nothing about
    /// whether a bare name is shadowed. The exclusion is part of the QUESTION
    /// rather than a test at the call site, so that two classes differing only
    /// in being `Object` cannot answer the same.
    ShadowsKernel(String),
    /// Does a `super` to `mname`, written in `defining_class`, take the VALUE
    /// channel? True only when the receiver is a value subclass whose ancestry
    /// holds `defining_class` and no user body above it -- `class Stack <
    /// Array` reaching the native `Array` method. Every other instance-method
    /// `super` emits the same runtime hand-off, which bakes `defining_class`
    /// and nothing about the receiver.
    ValueSuper {
        defining_class: ClassId,
        mname: String,
    },
    /// Is the receiver ancestry-related to `owner`, the class holding a
    /// `protected` target? Unrelated emits a `NoMethodError` where related
    /// emits the call.
    ProtectedRelated(ClassId),
    /// Is the receiver a class whose `self` is a bare `RubyValue` rather than a
    /// generated struct -- a reopened builtin, or `Object` itself? It picks a
    /// whole branch of implicit-self emission.
    ValueBacked,
    /// Does the receiver have a generated struct, and therefore a known ivar
    /// LAYOUT its shared body can index into?
    ///
    /// This is what `self_slots` means, and it is a property of the class, so a
    /// group holding both a user class and a reopened builtin splits on it
    /// rather than having to be kept apart by the grouping. A structless
    /// receiver reaches its ivars by name; a struct-backed one by slot.
    HasStruct,
}

/// What a [`ClassQuery`] answered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Answer {
    Slot(Option<u32>),
    Yes(bool),
}

impl Answer {
    /// The `bool` a yes/no query answered. Panics on a mismatched pairing,
    /// which would be a bug in this file rather than in a compiled program.
    pub(crate) fn yes(self) -> bool {
        match self {
            Answer::Yes(b) => b,
            other => unreachable!("a yes/no query answered {other:?}"),
        }
    }

    pub(crate) fn slot(self) -> Option<usize> {
        match self {
            Answer::Slot(s) => s.map(|s| s as usize),
            other => unreachable!("an ivar-slot query answered {other:?}"),
        }
    }
}

impl ClassQuery {
    /// THE evaluator. Both halves of the mechanism call it -- `Ctx::ask` while
    /// emitting, and [`Trace::agrees_for`] while replaying -- which is what
    /// makes a recorded answer a test another class can be held to.
    pub(crate) fn answer(&self, compiler: &Compiler, cid: ClassId) -> Answer {
        match self {
            ClassQuery::IvarSlot(name) => {
                Answer::Slot(slot_of(compiler, cid, name).map(|slot| slot as u32))
            }
            ClassQuery::InChain(name) => Answer::Yes(compiler.method_in_chain(cid, name).is_some()),
            ClassQuery::ShadowsKernel(name) => {
                Answer::Yes(cid != OBJECT_CLASS && compiler.method_in_chain(cid, name).is_some())
            }
            ClassQuery::ValueSuper {
                defining_class,
                mname,
            } => Answer::Yes(takes_value_super(compiler, cid, *defining_class, mname)),
            ClassQuery::ProtectedRelated(owner) => Answer::Yes(
                cid == *owner
                    || compiler.class(cid).ancestors.contains(owner)
                    || compiler.class(*owner).ancestors.contains(&cid),
            ),
            ClassQuery::ValueBacked => Answer::Yes(compiler.value_backed(cid)),
            ClassQuery::HasStruct => Answer::Yes(compiler.has_generated_struct(cid)),
        }
    }
}

/// Whether `super` to `mname` from a body written in `defining_class` reaches a
/// native value builtin rather than a user body -- see
/// [`ClassQuery::ValueSuper`]. Mirrors the test in `call::super_calls`.
fn takes_value_super(
    compiler: &Compiler,
    receiver: ClassId,
    defining_class: ClassId,
    mname: &str,
) -> bool {
    if !compiler.is_value_subclass(receiver) {
        return false;
    }
    let ancestors = &compiler.class(receiver).ancestors;
    let Some(pos) = ancestors.iter().position(|&a| a == defining_class) else {
        return false;
    };
    !ancestors[pos + 1..].iter().any(|&anc| {
        compiler
            .class(anc)
            .own_methods
            .iter()
            .any(|&sid| compiler.scope(sid).name == mname)
    })
}

/// The class a body's `self` is an instance of, together with where to record
/// what gets asked about it.
///
/// `captures` walks a body far from any `Ctx`, but the question it asks there --
/// does this class define `puts`, or is a bare `puts` the Kernel free function?
/// -- is a per-class question like any other, and it changes the shape of the
/// emitted closure. Carrying the class and the trace as one value is what stops
/// a caller from passing the first and forgetting the second.
#[derive(Clone, Copy, Default)]
pub(crate) struct SelfClass<'a> {
    pub(crate) cid: Option<ClassId>,
    trace: Option<&'a std::cell::RefCell<Trace>>,
}

impl<'a> SelfClass<'a> {
    pub(crate) fn new(
        cid: Option<ClassId>,
        trace: Option<&'a std::cell::RefCell<Trace>>,
    ) -> SelfClass<'a> {
        SelfClass { cid, trace }
    }

    /// [`super::Ctx::ask`] for code that has no `Ctx`. `None` when there is no
    /// receiver class, which is also when nothing can vary with one.
    pub(crate) fn ask(&self, compiler: &Compiler, query: ClassQuery) -> Option<Answer> {
        let cid = self.cid?;
        let answer = query.answer(compiler, cid);
        if let Some(trace) = self.trace {
            trace.borrow_mut().record(query, answer);
        }
        Some(answer)
    }
}

/// The questions one emission asked, in order, with the answers it got.
///
/// Order is not load-bearing -- replay checks every pair -- but keeping it lets
/// a divergence report name the first question that parted.
#[derive(Default, Clone)]
pub(crate) struct Trace(Vec<(ClassQuery, Answer)>);

impl Trace {
    pub(crate) fn record(&mut self, query: ClassQuery, answer: Answer) {
        // A body asks the same question repeatedly (one ivar read per mention).
        // Deduping keeps replay proportional to the DISTINCT class-dependence
        // of the body rather than to its length.
        if !self.0.iter().any(|(q, _)| q == &query) {
            self.0.push((query, answer));
        }
    }

    /// Whether `cid` answers every recorded question the same way -- i.e.
    /// whether the emission this trace came from is valid for `cid` too.
    pub(crate) fn agrees_for(&self, compiler: &Compiler, cid: ClassId) -> bool {
        self.0.iter().all(|(q, a)| q.answer(compiler, cid) == *a)
    }

    /// The first question `cid` answers differently, for diagnostics.
    pub(crate) fn first_disagreement(
        &self,
        compiler: &Compiler,
        cid: ClassId,
    ) -> Option<(&ClassQuery, Answer, Answer)> {
        self.0.iter().find_map(|(q, a)| {
            let theirs = q.answer(compiler, cid);
            (theirs != *a).then_some((q, *a, theirs))
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

/// The `IvarCell` slot index of `@name` on `class`: declared ivars first,
/// then hidden ivars (Struct/Data members) after them.
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
                // no longer in the live class-methods row (the prepend won), so
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

pub(crate) fn slot_of(compiler: &Compiler, class: ClassId, name: &str) -> Option<usize> {
    let info = compiler.class(class);
    info.ivars.iter().position(|iv| iv == name).or_else(|| {
        info.hidden_ivars
            .iter()
            .position(|iv| iv == name)
            .map(|i| info.ivars.len() + i)
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
