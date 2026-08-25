//! The method-implementation shapes: `MethodFn`/`DynMethodFn` and the
//! `MethodImpl` object-channel enum, the `ConstructorFn`/`AllocatorFn`
//! registration types, and `ValueMethodFn`/`ValueImpl` for the value channel.

use super::*;

/// Every generated method/trampoline returns `Result<RubyValue, Signal>`, not
/// a bare `RubyValue` -- see `Signal`'s docs for why this is fixed from the
/// start rather than retrofitted once `break`/`raise`/non-local `return`
/// exist. The third parameter is the call's block, if any (`None` when no
/// block was given) -- see `signal.rs`'s `catch_break` and `clif::blocks`'
/// docs for how a block crosses the dispatch boundary.
pub type MethodFn = fn(&RObj, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>;

/// A method body no bare `fn` pointer can represent: a closure carrying
/// captured state (a runtime `define_method` block today; interpreted eval-VM
/// bodies when that phase lands).
pub type DynMethodFn =
    Arc<dyn Fn(&RObj, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal> + Send + Sync>;

/// A registered instance method's implementation. Statically-registered
/// methods are `Static` -- a bare `fn` pointer -- so the hot path is a direct
/// indirect call with one folded discriminant branch, no allocation.
#[derive(Clone)]
pub enum MethodImpl {
    Static(MethodFn),
    // The runtime-metaprogramming seam: a compiled block captured as an
    // `Arc<dyn Fn>`, built by `runtime_meta::dynamic_from_proc` for a runtime
    // `define_method`. `Clone` is cheap on both arms (a `fn` copy / an `Arc`
    // bump) -- the overlay resolvers clone an entry out and drop their lock
    // BEFORE dispatching, so a runtime method that itself defines another
    // method can't deadlock.
    Dynamic(DynMethodFn),
    /// A Cranelift-compiled body registered on the object channel (an
    /// `ObjRow` -- the CLIF twin of `ruby_class!`'s `dispatch{}` rows).
    /// Called with a borrowed `RubyValue::Object` VIEW of the receiver:
    /// `ManuallyDrop` over a `ptr::read` copy, so no Arc bump on the hot
    /// path -- sound because the view lives only for the call and the
    /// callee only borrows its receiver.
    CValue(crate::capi::ValueFn),
}

impl MethodImpl {
    #[inline(always)]
    pub fn call(
        &self,
        recv: &RObj,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        match self {
            MethodImpl::Static(f) => f(recv, args, block),
            MethodImpl::Dynamic(f) => f(recv, args, block),
            MethodImpl::CValue(f) => {
                let view =
                    std::mem::ManuallyDrop::new(RubyValue::Object(unsafe { std::ptr::read(recv) }));
                crate::capi::dispatch::call_value_fn(*f, &view, args, block)
            }
        }
    }

    #[inline(always)]
    pub fn from_fn(f: MethodFn) -> Self {
        MethodImpl::Static(f)
    }
}

/// A class's dynamic constructor: allocates a fresh instance
/// and runs its `initialize` (if any) -- what makes `x = Widget;
/// x.new(...)` work when the class is only known at runtime as a
/// `RubyValue::Class` value. Generated per class by `ruby_class!`
/// (`__construct`, which ignores the id and uses its own `Self::CLASS_ID`);
/// `None` for modules and builtins.
///
/// The leading `ClassId` lets ONE constructor back many classes -- the native
/// exception hierarchy registers every exception class with the same
/// `RubyException`-allocating fn, which reads the id from here instead of a
/// per-class Rust type (see `crate::builtins::exception`).
pub type ConstructorFn = fn(ClassId, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>;

/// Allocates a fresh, ZERO-INITIALIZED instance of a class WITHOUT running
/// `initialize` -- backs `Class#allocate`. Mirrors `ConstructorFn`'s
/// id-passing shape (one allocator can back many classes) but returns the
/// bare `RObj` (every ivar `Nil`, frozen `false`); the caller wraps it in
/// `RubyValue::Object`. `None` for modules/builtins (which have no generated
/// struct); `Class#allocate` on those either yields a builtin empty value or
/// raises, handled at the call site.
pub type AllocatorFn = fn(ClassId) -> RObj;

/// A method defined by REOPENING a builtin class (`class String;
/// def blank?; ...`): the receiver is the builtin VALUE itself (`&RubyValue`,
/// not an `RObj` -- builtins have no generated struct to downcast to), which
/// is exactly what the generated free-function bodies take as `__self`.
/// Consulted FIRST by `send_value` (before even the universal `==`/`dup`
/// arms and the curated tables), because a user redefinition must OVERRIDE
/// the builtin behavior -- real Ruby's rule, oracle-verified (`class String;
/// def length; 42; end` wins everywhere). Per-box overlays key off
/// this same table.
pub type ValueMethodFn =
    fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>;

/// One value-channel implementation: the [`ValueMethodFn`] shape every
/// Rust row registers as, or a Cranelift-compiled C body. What the
/// registry's value tables, `FlatHit`, and the inline caches store --
/// `Copy` (two words) so caches keep holding it by value; `call` adds one
/// predictable branch over the direct call it replaces. The
/// generated-code-facing `define_*` signatures keep taking bare
/// `ValueMethodFn` (their row-table types are baked into emitted programs)
/// and wrap into `Rust` at the insertion point; C rows arrive through the
/// `_c` twins.
#[derive(Clone, Copy)]
pub enum ValueImpl {
    Rust(ValueMethodFn),
    C(crate::capi::ValueFn),
}

impl ValueImpl {
    #[inline(always)]
    pub fn call(
        &self,
        recv: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        match self {
            ValueImpl::Rust(f) => f(recv, args, block),
            ValueImpl::C(f) => crate::capi::dispatch::call_value_fn(*f, recv, args, block),
        }
    }

    /// The closure shape `RProc::with_self_and_block` and friends take --
    /// for the wrap sites where a bare `ValueMethodFn` used to pass as an
    /// `impl Fn` directly.
    pub(crate) fn into_fn(
        self,
    ) -> impl Fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal> {
        move |recv, args, block| self.call(recv, args, block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both `MethodImpl` arms invoke through `call` with the same ABI -- the
    /// widening a run-time `eval` depends on, verified without any registry.
    #[test]
    fn method_impl_static_and_dynamic_dispatch_through_call() {
        let recv: RObj = Arc::new(Object::default());

        fn stat(_: &RObj, _: &[RubyValue], _: Option<RubyValue>) -> Result<RubyValue, Signal> {
            Ok(RubyValue::Int(1))
        }
        let s = MethodImpl::from_fn(stat);
        assert!(matches!(s.call(&recv, &[], None), Ok(RubyValue::Int(1))));

        let d = MethodImpl::Dynamic(Arc::new(
            |_: &RObj, _: &[RubyValue], _: Option<RubyValue>| Ok(RubyValue::Int(2)),
        ));
        assert!(matches!(d.call(&recv, &[], None), Ok(RubyValue::Int(2))));
    }
}
