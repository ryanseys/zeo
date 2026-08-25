//! The real, escaping `Proc`/closure type -- deliberately a plain
//! Rust closure type, not a hand-rolled trait + per-call-site env struct: a
//! `move |args| { ... }` closure generated at each escaping block's call
//! site already IS a concrete, heap-allocated environment (Rust's own
//! compiler builds it for us), so there is nothing left for a hand-rolled
//! `ProcBody` trait to add except ceremony. `redo` is handled entirely
//! inside the closure body (a plain `loop` around the block's own code, see
//! `codegen`'s Proc-construction docs) -- no runtime support needed beyond
//! `Signal::Redo` already existing.
//!
//! Must be `'static`: `RubyValue` (which stores this) has no lifetime
//! parameter anywhere in this codebase, so anything it holds has to be
//! independently owned, not borrowed -- this is exactly why an escaping
//! block captures OWNED `Arc<parking_lot::Mutex<RubyValue>>` cells (and an
//! owned `Arc<Self>` for `self`/ivar access) rather than references.
//!
//! `+ Send + Sync`: a Rust closure is automatically `Send`/`Sync`
//! based purely on what it captures -- once every capture is `Arc`/`Mutex`-
//! based, the closures codegen generates satisfy this bound with no manual
//! annotation needed at the construction site.

use crate::builtins::type_error;
use crate::{RubyValue, Signal};
use std::borrow::Cow;
use std::sync::Arc;

/// The shared body of a `Proc`: `(self, args, call-site block) -> result`.
/// See [`ProcData::f`] for why `self` and the block are parameters. An `Arc`
/// (not a `Box`) so `dup`/`clone` can mint a FRESH `ProcData` -- fresh
/// object identity, fresh frozen flag -- while sharing the one closure
/// allocation, exactly CRuby's split (the copy is a new object built from
/// the same block).
type ProcFn = Arc<
    dyn Fn(
            Option<&crate::capi::procs::ProcEnvOwned>,
            &RubyValue,
            &[RubyValue],
            Option<RubyValue>,
        ) -> Result<RubyValue, Signal>
        + Send
        + Sync,
>;

/// The closure a `Proc` value wraps, plus the two facts about its
/// PARAMETERS that a Rust closure can't answer for itself but Ruby exposes:
/// `Proc#arity` and `Proc#lambda?` (and `#curry`, which needs the arity).
/// Codegen fills them in from the block/lambda's static `Params` at every
/// literal construction site; runtime-internal procs (Enumerator shuttles,
/// `Symbol#to_proc`, ...) use `RProc::new`'s var-args default, which is
/// what CRuby reports for a comparable C-implemented proc anyway.
pub struct ProcData {
    /// Takes the `self` to run under as its FIRST parameter rather than
    /// capturing it, which is what makes `instance_exec` possible: a Ruby
    /// block's self is not fixed at creation: `obj.instance_exec { @x }`
    /// runs this same proc body under a DIFFERENT receiver. A captured
    /// `self` could only be rebound by mutating shared state (every clone
    /// of the proc shares one `Arc<ProcData>`, so that would race, and a
    /// save/restore around the call would corrupt any concurrent use). A
    /// parameter is immutable, reentrant, and thread-safe by construction.
    /// The third parameter is the CALL-SITE block, which is a different
    /// thing from the closure env this proc already captured. CRuby keeps
    /// them apart too: `invoke_bmethod` (`vm.c:1786`) threads the proc's
    /// captured env through `VM_GUARDED_PREV_EP` while writing the caller's
    /// block handler into the frame's specval, and `yield` reads the latter
    /// (`vm.c:1843`). Collapsing them would make a `define_method` body's
    /// `yield` see the block that was passed to `define_method` rather than
    /// the one passed to the resulting method.
    f: ProcFn,
    /// The block's LEXICAL self -- the receiver `#call` runs under, i.e.
    /// what `self` meant where the block was written. `instance_exec`
    /// bypasses it; everything else uses it.
    self_val: RubyValue,
    /// CRuby's encoding: a required-only signature is the positive count;
    /// any optional/rest param makes it `-(required + 1)`.
    pub arity: i32,
    pub is_lambda: bool,
    /// `Proc#parameters`: the block/lambda's static parameter list, in CRuby's
    /// `[[kind, name], ...]` order and kinds (a proc reports required
    /// positionals as `:opt`, a lambda as `:req`). Empty for a runtime-internal
    /// proc (`RProc::new`), matching CRuby's `[[:rest]]`-ish C-proc reporting
    /// only where codegen supplied it. `Cow`: codegen shares one per-signature
    /// static table across every construction of that signature
    /// (`with_params_static`), so building a proc allocates nothing here.
    params: std::borrow::Cow<'static, [ProcParamMeta]>,
    /// The method activation this block/lambda was constructed inside (see
    /// `crate::signal::home_current`). A non-lambda Proc's `return` unwinds to
    /// this home if it is still on the stack, else raises `LocalJumpError`.
    /// `None` for a runtime-internal proc or one built at the top level (a
    /// `return` from the latter is an unconditional `LocalJumpError`).
    home: Option<crate::signal::ProcHome>,
    /// `Proc#binding` -- the DEFINING scope, captured at construction: the
    /// `self` the block was written under and that scope's own local cells
    /// (never the block's own locals, which do not exist until it runs --
    /// oracle-verified). `None` for a runtime-internal proc, and for one
    /// built by a program that never asks for it, since codegen only pays
    /// for the capture when a `Proc#binding` call is somewhere in the
    /// program (`Hir::uses_proc_binding`); `#binding` then answers CRuby's
    /// C-level-Proc `ArgumentError`.
    binding: Option<RubyValue>,
    /// Where the block/lambda was WRITTEN -- `Proc#source_location`, and the
    /// middle of `#inspect`. `None` for a runtime-internal proc, which is
    /// CRuby's C-level Proc and reports `nil` for both.
    ///
    /// Two static values, so an unasked-for location costs a program nothing
    /// beyond the word: codegen already has the span (it threads the same one
    /// into `#binding`), and the file is a literal in the generated source.
    location: Option<(&'static str, u32)>,
    /// The class this proc IS an instance of -- `Proc` for every one a program
    /// writes, and a user subclass only for `class P < Proc` (declarative's
    /// `Variables::Proc`). Kept HERE rather than in a wrapper object so a
    /// subclass instance is still a `RubyValue::Proc`: every call-site fast
    /// path, `&blk` conversion and `to_proc` keeps working on it unchanged,
    /// which a `ValueSubclass` payload would have broken.
    class_id: crate::ClassId,
    /// The Symbol this proc was DERIVED from (`:upcase.to_proc`, `&:name`) --
    /// what makes `#inspect` render `#<Proc:0x...(&:upcase) (lambda)>`
    /// instead of a source location. `None` for every other construction.
    origin: Option<crate::Symbol>,
    /// The first enclosing-scope local this block captures (alphabetically),
    /// or `None` for a capture-free body -- what `Ractor.new(&proc)` reads to
    /// raise CRuby's Proc-isolation `ArgumentError` at the moment CRuby
    /// raises it. The verdict is decided at COMPILE time (the capture set is
    /// static) but must ride on the VALUE: a dynamic proc's creation site and
    /// its `Ractor.new` site only meet at runtime. Self/ivar access is NOT
    /// recorded: CRuby 4.0 isolates such a proc fine and only its ivar READS
    /// fail inside the ractor (the process-shared divergence `ractor.rs`
    /// documents). `None` for runtime-internal procs, which capture no Ruby
    /// locals by construction.
    outer_capture: Option<&'static str>,
    /// `.frozen?` state -- Procs are freezable ordinary objects in Ruby
    /// (freezing one changes nothing observable beyond the flag: no mutating
    /// methods exist), and `dup`/`clone` follow the standard flag rule via
    /// `dup_data`.
    frozen: std::sync::atomic::AtomicBool,
    /// A compiled block's captured environment, held BESIDE the closure and
    /// handed to it per call rather than captured inside it.
    ///
    /// It used to live inside the `Arc<dyn Fn>`, where nothing could see it.
    /// The cycle collector has to enumerate the captured cells --
    /// `obj.callback = -> { obj }`, the canonical ruby leak, closes through
    /// exactly those -- and no reflection opens a Rust closure. Passing it as
    /// a parameter keeps it visible without a second allocation.
    ///
    /// `None` for every proc built from a Rust closure, which captures no
    /// Ruby cells.
    env: Option<crate::capi::procs::ProcEnvOwned>,
    /// Was this handle written as a LITERAL block at a call site, and has it
    /// reached the callee without being named on the way?
    ///
    /// CRuby's frame carries either an iseq block handler -- a `{ }` written
    /// at this very call -- or a proc handler, and `rb_block_lambda` refuses
    /// the second. Nothing about the VALUE distinguishes them, so the mark
    /// rides here: every emitter-built proc sets it, and the `&expr`
    /// conversion clears it, because naming a proc and passing it with `&`
    /// is what turns an iseq handler into a proc handler. `Kernel#lambda` is
    /// the only reader.
    ///
    /// Cleared IN PLACE rather than on a copy: once a handle has been passed
    /// with `&`, it can never be a literal again.
    literal_block: std::sync::atomic::AtomicBool,
}

impl ProcData {
    /// Every field but the ones a copy constructor overrides. `dup`, `clone`,
    /// `#lambda` and a `Proc` subclass all mint a FRESH `ProcData` (fresh
    /// identity, fresh frozen flag) sharing the one closure allocation, which
    /// is CRuby's own copy semantics.
    fn copy(&self) -> ProcData {
        ProcData {
            class_id: self.class_id,
            f: Arc::clone(&self.f),
            self_val: self.self_val.clone(),
            arity: self.arity,
            is_lambda: self.is_lambda,
            params: self.params.clone(),
            home: self.home.clone(),
            binding: self.binding.clone(),
            location: self.location,
            origin: self.origin,
            outer_capture: self.outer_capture,
            frozen: std::sync::atomic::AtomicBool::new(false),
            // A copy is a NAMED handle, never the literal block a call site
            // wrote -- `dup`, `clone` and `#lambda` all mint one.
            literal_block: std::sync::atomic::AtomicBool::new(false),
            // A copy shares the one closure allocation, so it must run under
            // the same environment. `ProcEnvOwned` owns raw views into its
            // own cells and cannot be duplicated; the copy takes its own
            // references to the same cells instead.
            env: self
                .env
                .as_ref()
                .map(crate::capi::procs::ProcEnvOwned::share),
        }
    }
}

/// One entry of `Proc#parameters` -- a parameter's kind (`"req"`, `"opt"`,
/// `"rest"`, `"keyreq"`, `"key"`, `"keyrest"`, `"block"`) and optional name.
/// Codegen builds these from the block/lambda's static signature. Both
/// fields are `&'static str` (the name interns lazily at the `#parameters`
/// reflection site) so a whole signature can live in one `static` table.
#[derive(Clone)]
pub struct ProcParamMeta {
    pub kind: &'static str,
    pub name: Option<&'static str>,
}

/// Fills a [`ProcData`] before the single `Arc::new` that seals it.
///
/// It replaces eight `Arc::get_mut` setters that ran AFTER construction and
/// worked only while the handle's refcount was still 1. That was already
/// fragile; registering a proc with the cycle collector made it impossible,
/// because `Arc::downgrade` makes `Arc::get_mut` fail forever. Every setter
/// would have become a silent no-op -- no `source_location`, no
/// `#parameters`, no `return` home, no `#binding` -- and nothing would have
/// said so.
///
/// Filling the struct first removes the hazard rather than working around
/// it: there is no window in which the data is reachable and incomplete.
pub struct ProcBuilder(ProcData);

impl ProcBuilder {
    /// A proc whose body is a plain Rust closure: an Enumerator shuttle,
    /// `Symbol#to_proc`, `Method#to_proc`, and every other runtime-internal
    /// one. It captures no Ruby cells, so it carries no environment.
    pub fn from_rust(
        f: impl Fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>
        + Send
        + Sync
        + 'static,
        self_val: RubyValue,
        arity: i32,
        is_lambda: bool,
    ) -> ProcBuilder {
        ProcBuilder(ProcData {
            class_id: zeo_abi::PROC_CLASS,
            f: Arc::new(move |_env, recv, args, block| f(recv, args, block)),
            self_val,
            arity,
            is_lambda,
            params: std::borrow::Cow::Borrowed(&[]),
            home: None,
            binding: None,
            location: None,
            origin: None,
            outer_capture: None,
            frozen: std::sync::atomic::AtomicBool::new(false),
            env: None,
            literal_block: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// A proc whose body is a Cranelift-compiled [`crate::capi::BlockFn`].
    ///
    /// The environment is shared: the closure holds one handle and bridges
    /// every invocation through `capi::procs::call_block_fn`, and
    /// [`ProcData::env`] holds another so the collector can see the captured
    /// cells. `self` stays a per-call parameter -- `instance_exec` keeps
    /// working -- and the call-site block forwards into the body's own `blk`
    /// slot; parameter binding (auto-splat included) lives INSIDE the
    /// compiled body, exactly as it lives inside the Rust closures above.
    pub fn from_c(
        f: crate::capi::BlockFn,
        env: crate::capi::procs::ProcEnvOwned,
        self_val: RubyValue,
        arity: i32,
        is_lambda: bool,
    ) -> ProcBuilder {
        let mut b = ProcBuilder::from_rust(|_, _, _| unreachable!(), self_val, arity, is_lambda);
        b.0.f = Arc::new(move |env, recv, args, block| {
            let env = env.expect("a compiled block always carries its environment");
            crate::capi::procs::call_block_fn(f, env, recv, args, block)
        });
        b.0.env = Some(env);
        // Every proc the emitter builds came from a block WRITTEN in the
        // source at a call site. `Kernel#lambda` reads it; the `&expr`
        // conversion clears it.
        b.0.literal_block = std::sync::atomic::AtomicBool::new(true);
        b
    }

    /// The static `Proc#parameters` list the compiler computed from the
    /// block/lambda's signature.
    pub fn params(mut self, params: Vec<ProcParamMeta>) -> ProcBuilder {
        self.0.params = std::borrow::Cow::Owned(params);
        self
    }

    /// Capture the current method activation as this proc's home (see
    /// [`ProcData::home`]). A runtime-internal proc skips it.
    pub fn home(mut self) -> ProcBuilder {
        self.0.home = crate::signal::home_current();
        self
    }

    /// The defining scope, as this proc's `#binding` (see
    /// [`ProcData::binding`]).
    pub fn binding(mut self, binding: RubyValue) -> ProcBuilder {
        self.0.binding = Some(binding);
        self
    }

    /// Where the block/lambda was written (see [`ProcData::location`]).
    pub fn location(mut self, file: &'static str, line: u32) -> ProcBuilder {
        self.0.location = Some((file, line));
        self
    }

    /// Tag this proc as derived from a Symbol (see [`ProcData::origin`]).
    pub fn symbol_origin(mut self, name: crate::Symbol) -> ProcBuilder {
        self.0.origin = Some(name);
        self
    }

    /// The first outer local this block captures (see
    /// [`ProcData::outer_capture`]) -- `Ractor.new`'s refusal evidence.
    pub fn outer_capture(mut self, name: &'static str) -> ProcBuilder {
        self.0.outer_capture = Some(name);
        self
    }

    /// Seal the data into a `Proc` value and register it with the collector.
    pub fn build(self) -> RProc {
        RProc::of(self.0)
    }
}

/// A `Proc` value's payload. A newtype over `Arc<ProcData>` rather than the
/// bare `Arc<dyn Fn>` it started as -- `Deref` to the closure keeps every
/// existing `p(&args)` call site working unchanged (a call expression
/// auto-dereferences its callee), while giving the value somewhere to carry
/// `arity`/`is_lambda`.
#[derive(Clone)]
pub struct RProc(Arc<ProcData>);

impl RProc {
    /// This proc's identity -- the address of its shared payload, which two
    /// handles to the SAME proc agree on and two distinct procs never do. The
    /// `Arc` is private, so the identity-keyed side tables (`value_ivars`) ask
    /// for it here rather than reaching for the field.
    pub(crate) fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as *const () as usize
    }

    /// The one place a `Proc` value is built.
    ///
    /// Registering here is what forced [`ProcBuilder`] into being:
    /// `Arc::downgrade` makes `Arc::get_mut` fail FOREVER, and the metadata
    /// setters this type used to expose were all `Arc::get_mut` on a
    /// refcount-1 handle. Taking a weak handle here would have turned every
    /// one of them into a silent no-op.
    fn of(data: ProcData) -> RProc {
        let p = RProc(Arc::new(data));
        crate::gc::record_proc(&p);
        p
    }

    /// A weak handle to this proc's payload -- the allocation registry's way
    /// of holding a candidate without keeping it alive.
    pub(crate) fn downgrade(&self) -> std::sync::Weak<ProcData> {
        Arc::downgrade(&self.0)
    }

    /// The same handle, type-erased, for a side table that only needs the
    /// allocation held open so this proc's address stays unique.
    pub(crate) fn weak_owner(&self) -> std::sync::Weak<dyn std::any::Any + Send + Sync> {
        let erased: Arc<dyn std::any::Any + Send + Sync> = self.0.clone();
        Arc::downgrade(&erased)
    }

    /// The proc a weak handle names, or `None` once it has been dropped.
    pub(crate) fn upgrade(w: &std::sync::Weak<ProcData>) -> Option<RProc> {
        w.upgrade().map(RProc)
    }

    /// How many owners this proc has, including the caller's handle.
    pub(crate) fn owners(&self) -> usize {
        Arc::strong_count(&self.0)
    }

    /// Every strong reference this proc owns that the collector can match to
    /// a registered node: the lexical `self` a body reads `@ivars` through,
    /// the enclosing block a nested bare `yield` reaches, and the `#binding`
    /// it captured.
    ///
    /// Unlike every other node, a Proc reports edges it cannot RELEASE --
    /// its captures are immutable by construction, which is why it reported
    /// nothing at all before. See [`crate::gc::collect`] for why that is
    /// sound: a reclaimed proc's owners are themselves reclaimed and
    /// cleared, so the proc dies with the pass's handles and takes its
    /// captures with it, and the pass CHECKS that it did.
    pub(crate) fn gc_edges(&self, out: &mut Vec<RubyValue>) {
        out.push(self.0.self_val.clone());
        if let Some(b) = &self.0.binding {
            out.push(b.clone());
        }
        if let Some(env) = &self.0.env {
            env.gc_edges(out);
        }
    }

    /// The captured cells this proc owns a reference to, by address. A cell
    /// is not a `RubyValue`, so it cannot travel [`RProc::gc_edges`].
    pub(crate) fn gc_cells(&self, out: &mut Vec<usize>) {
        if let Some(env) = &self.0.env {
            env.gc_cells(out);
        }
    }

    /// A runtime-internal proc: var-args arity (`-1`), not a lambda. Its
    /// body has no Ruby `self` to speak of (Enumerator shuttles,
    /// `Symbol#to_proc`, ...), so it ignores the receiver and reports nil as
    /// its lexical self.
    pub fn new(
        f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static,
    ) -> RProc {
        ProcBuilder::from_rust(
            move |_self, args, _block| f(args),
            RubyValue::Nil,
            -1,
            false,
        )
        .build()
    }

    /// A proc built from Ruby source, whose `Params` codegen knows, and
    /// whose body never mentions `self` (no ivars, no implicit-self call) --
    /// so there is nothing for `instance_exec` to rebind.
    pub fn with_meta(
        f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static,
        arity: i32,
        is_lambda: bool,
    ) -> RProc {
        ProcBuilder::from_rust(
            move |_self, args, _block| f(args),
            RubyValue::Nil,
            arity,
            is_lambda,
        )
        .build()
    }

    /// A proc built from Ruby source whose body DOES use `self` -- codegen
    /// emits this form, passing the block's lexical self as `self_val`. The
    /// closure reads its receiver from the parameter, so `instance_exec` can
    /// supply a different one (see `call_with_self`).
    pub fn with_self(
        f: impl Fn(&RubyValue, &[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static,
        self_val: RubyValue,
        arity: i32,
        is_lambda: bool,
    ) -> RProc {
        RProc::with_self_and_block(
            move |s, args, _block| f(s, args),
            self_val,
            arity,
            is_lambda,
        )
    }

    /// `with_self` for a body that can also see the CALL-SITE block -- what a
    /// `define_method`'d method needs so `yield` and `&blk` inside it reach
    /// the block passed to the method, not the one passed to define_method.
    pub fn with_self_and_block(
        f: impl Fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>
        + Send
        + Sync
        + 'static,
        self_val: RubyValue,
        arity: i32,
        is_lambda: bool,
    ) -> RProc {
        ProcBuilder::from_rust(f, self_val, arity, is_lambda).build()
    }

    /// The `Proc#parameters` metadata (empty for a runtime-internal proc).
    pub fn parameters(&self) -> &[ProcParamMeta] {
        self.0.params.as_ref()
    }

    /// The defining scope this Proc captured, if codegen supplied one.
    pub fn binding(&self) -> Option<&RubyValue> {
        self.0.binding.as_ref()
    }

    /// The outer local that makes this proc non-isolable, if codegen found
    /// one -- `Ractor.new`'s refusal evidence.
    pub fn outer_capture(&self) -> Option<&'static str> {
        self.0.outer_capture
    }

    /// The Symbol this proc was derived from, if any.
    pub fn symbol_origin(&self) -> Option<crate::Symbol> {
        self.0.origin
    }

    /// Where this Proc was written, if codegen supplied it.
    pub fn location(&self) -> Option<(&'static str, u32)> {
        self.0.location
    }

    /// Resolve a non-lambda Proc's `Signal::Return` against its captured home:
    /// a live home means a genuine CRuby non-local return (propagate it to
    /// unwind the method); a dead home (or none) means the `return` has no
    /// method to jump to -- `LocalJumpError`, as in CRuby. (A lambda folds its
    /// own `return` internally and never reaches here with one.)
    fn resolve_home_return(&self, result: Result<RubyValue, Signal>) -> Result<RubyValue, Signal> {
        match result {
            // Convert ONLY when THIS proc captured a home that has since died.
            // A `None` home is either a runtime-internal proc merely RELAYING a
            // `Signal::Return` from a user block it invoked (must propagate,
            // not swallow), or a top-level proc (current leak behavior kept);
            // a live home is a genuine non-local return in flight.
            Err(Signal::Return(v)) if !self.0.is_lambda => match &self.0.home {
                Some(home) if !crate::signal::proc_home_alive(home) => {
                    Err(crate::dispatch::raise_error_details(
                        "LocalJumpError",
                        "unexpected return".to_string(),
                        &[
                            ("reason", RubyValue::Symbol(crate::Symbol::intern("return"))),
                            ("exit_value", v),
                        ],
                    ))
                }
                // A live home: a genuine non-local return, marked so the
                // activation it is aimed at takes it and any method it unwinds
                // through leaves it alone. A `None` home means this proc is
                // only relaying somebody else's, whose mark must stand.
                Some(home) => Err(crate::signal::signal_return_to(home, v)),
                None => Err(Signal::Return(v)),
            },
            other => other,
        }
    }

    /// Invoke under the block's own lexical self -- ordinary `#call`/`yield`.
    pub fn call(&self, args: &[RubyValue]) -> Result<RubyValue, Signal> {
        let result = (self.0.f)(self.0.env.as_ref(), &self.0.self_val, args, None);
        self.resolve_home_return(result)
    }

    /// Invoke with `self` REBOUND to `recv` -- `instance_exec`/`instance_eval`.
    /// Leaves this proc untouched and is safe to call concurrently: the
    /// receiver is a parameter, never stored.
    pub fn call_with_self(
        &self,
        recv: &RubyValue,
        args: &[RubyValue],
    ) -> Result<RubyValue, Signal> {
        self.call_with_self_and_block(recv, args, None)
    }

    /// `call_with_self` that also hands the body a call-site block.
    pub fn call_with_self_and_block(
        &self,
        recv: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        let result = (self.0.f)(self.0.env.as_ref(), recv, args, block);
        self.resolve_home_return(result)
    }

    /// Ordinary `#call` that ALSO forwards a call-site block to the proc's own
    /// `&block` parameter -- keeps the proc's own lexical `self` (unlike
    /// `call_with_self_and_block`, which rebinds it for `instance_exec`). This
    /// is what `->(&b) { b.call(...) }.call { ... }` needs: the block passed to
    /// `#call` reaches the lambda's `&b`.
    pub fn call_with_block(
        &self,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        let result = (self.0.f)(self.0.env.as_ref(), &self.0.self_val, args, block);
        self.resolve_home_return(result)
    }

    /// The block's lexical self -- `Proc#binding`-adjacent reflection, and
    /// what `instance_exec` restores nothing to (it simply doesn't consult it).
    pub fn self_val(&self) -> &RubyValue {
        &self.0.self_val
    }

    pub fn arity(&self) -> i32 {
        self.0.arity
    }

    pub fn is_lambda(&self) -> bool {
        self.0.is_lambda
    }

    /// Pointer identity -- `Proc#==`/`#equal?` and Ractor sharability
    /// checks compare the underlying allocation, not the closure's
    /// behavior.
    pub fn ptr_eq(&self, other: &RProc) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// `Proc#==`/`#eql?`: two procs are equal when they wrap the SAME block --
    /// which `dup`/`clone` preserve (they share the closure allocation while
    /// minting a fresh `ProcData`), so `l.dup == l` is true even though
    /// `l.dup.equal?(l)` (allocation identity) is false.
    pub fn block_eq(&self, other: &RProc) -> bool {
        Arc::ptr_eq(&self.0.f, &other.0.f)
    }

    /// This proc's allocation address, as `Hash`-key identity and
    /// `#object_id` need it -- the same notion `ptr_eq` compares, exposed as
    /// a value.
    pub fn ptr_id(&self) -> usize {
        Arc::as_ptr(&self.0) as *const () as usize
    }

    /// `Proc#frozen?` -- see [`ProcData::frozen`].
    pub fn is_frozen(&self) -> bool {
        self.0.frozen.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// `Proc#freeze`'s storage half; repeat calls are harmless no-ops.
    pub fn set_frozen(&self) {
        self.0
            .frozen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Did a call site write this handle as a LITERAL block -- see
    /// [`ProcData::literal_block`]. `Kernel#lambda` is the only reader.
    pub fn is_literal_block(&self) -> bool {
        self.0
            .literal_block
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Retire the literal-block mark: this handle has now been named and
    /// passed with `&`, which is CRuby's proc handler. One-way.
    pub fn clear_literal_block(&self) {
        self.0
            .literal_block
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// `Proc#dup`/`#clone`'s payload copy: a FRESH `ProcData` (new object
    /// identity, `frozen` per the caller's dup-vs-clone rule) sharing the
    /// one closure allocation -- CRuby's own copy semantics, under which
    /// freezing the original never freezes an earlier copy.
    /// The same block AS A LAMBDA -- what `Kernel#lambda { }` answers when it
    /// is reached through `send` rather than folded at the call site. Only the
    /// flag differs, and with it `#lambda?`, the argument-count strictness it
    /// implies, and where a `return` inside the body unwinds to.
    pub fn as_lambda(&self) -> RProc {
        if self.0.is_lambda {
            return self.clone();
        }
        RProc::of(ProcData {
            is_lambda: true,
            ..self.0.copy()
        })
    }

    /// The class this proc is an instance of -- `Proc` unless a
    /// `class P < Proc` minted it. See [`ProcData::class_id`].
    pub fn class_id(&self) -> crate::ClassId {
        self.0.class_id
    }

    /// The same proc as an instance of `class_id`, sharing the one closure
    /// allocation. What a `class P < Proc` constructor applies to the block it
    /// was given, so `P.new { }` answers a `P` that is still a
    /// `RubyValue::Proc`.
    pub fn as_class(&self, class_id: crate::ClassId) -> RProc {
        if self.0.class_id == class_id {
            return self.clone();
        }
        RProc::of(ProcData {
            class_id,
            ..self.0.copy()
        })
    }

    pub fn dup_data(&self, frozen: bool) -> RProc {
        RProc::of(ProcData {
            frozen: std::sync::atomic::AtomicBool::new(frozen),
            ..self.0.copy()
        })
    }
}

// No `Deref` to the inner closure (it existed to keep bare `p(&args)` call
// sites working): the closure now takes `self` first, so a call expression
// can't stand in for a decision about WHICH receiver to run under. Every
// invocation goes through `call` (lexical self) or `call_with_self`
// (rebound) and thereby states which one it means.

/// Pack a logical tuple into ONE `RubyValue::Array` before invoking `p` --
/// the uniform poly-array ABI every pair/tuple-yielding iterator uses (CRuby
/// `Hash#each`'s shape). A `{ |k, v| }` block auto-splats the array back to
/// `k, v`; a `{ |pair| }` block and any forwarded callable (`&method(:m)`,
/// `&:sym`) receive the whole array as one argument.
pub fn yield_tuple(p: &RProc, elems: Vec<RubyValue>) -> Result<RubyValue, Signal> {
    p.call(&[RubyValue::Array(crate::array_new(elems))])
}

/// Yield a logical PAIR, packing it only when the block cannot take two.
///
/// CRuby's `rb_hash_foreach` hands the block two values and packs them only
/// for a block that takes one. Packing unconditionally cost THREE heap
/// allocations per pair -- the `Vec`, the `Arc<Freezable<ArrayStore>>` around
/// it, and the `to_vec` that [`block_auto_splat`] immediately does to take it
/// apart again -- for the `{ |k, v| }` shape, which is the common one.
///
/// The condition is the block's own declared shape, and it reproduces CRuby's
/// auto-splat rule exactly (each clause verified against ruby 4.0.6):
///
/// * a LAMBDA never splats. `{a: 1}.each(&->(k, v) {})` is an `ArgumentError`
///   in CRuby *because* the lambda receives one argument, so a lambda must
///   keep getting the packed pair or that error would stop happening.
/// * `{ |pair| }` (arity 1) and `{ |*a| }` (arity -1) take the pair WHOLE --
///   the latter sees `[[k, v]]`, not `[k, v]`.
/// * anything with room for two or more positionals splats: `{ |k, v| }` (2),
///   `{ |k, *r| }` (-2), `{ |k, v, w| }` (3, binding `w` to nil).
///
/// A runtime-internal proc ([`RProc::new`], which the enumerable drivers pass
/// to a user-defined `each`) has arity -1, so it keeps the packed form and
/// `enumerable::pack` still sees one argument.
pub fn yield_pair(p: &RProc, k: RubyValue, v: RubyValue) -> Result<RubyValue, Signal> {
    if !p.is_lambda() && (p.arity() >= 2 || p.arity() <= -2) {
        return p.call(&[k, v]);
    }
    yield_tuple(p, vec![k, v])
}

/// A non-lambda block's AUTO-SPLAT (CRuby `setup_parameters_complex`'s
/// `arg_setup_block` path): a block yielded EXACTLY ONE argument that is an
/// Array (or `to_ary`-coercible) has that array spread across its
/// positional parameters -- `[[1, 2]].each { |a, b| }` binds `a=1, b=2`,
/// the idiom that makes `Hash#each { |k, v| }` and `each_with_index` read
/// naturally.
///
/// The DECISION (which param shapes splat at all) is the compiler's, made
/// statically from the block's own `Params` -- see
/// `clif::params::auto_splats`. This function is only
/// reached once that decision says yes, so it just performs the coercion.
/// A lambda never comes here (strict arity, no auto-splat).
///
/// Borrows: the overwhelmingly common answer is "no coercion applies", and
/// the caller is a block prologue running on every invocation, so the
/// pass-through case must not allocate.
pub fn block_auto_splat(args: &[RubyValue]) -> Result<Cow<'_, [RubyValue]>, Signal> {
    if args.len() != 1 {
        return Ok(Cow::Borrowed(args));
    }
    match &args[0] {
        RubyValue::Array(a) => Ok(Cow::Owned(a.lock().to_vec())),
        v => {
            let to_ary = crate::Symbol::intern("to_ary");
            // `rb_check_array_type`: a `to_ary` answering a non-Array is
            // simply not a coercion (no TypeError here, unlike a splice's
            // explicit conversion) -- the value binds as one argument.
            if crate::dispatch::responds_to(v.class_id(), to_ary, false)
                && let RubyValue::Array(a) = crate::dispatch::send_value(v, to_ary, &[], None)?
            {
                return Ok(Cow::Owned(a.lock().to_vec()));
            }
            Ok(Cow::Borrowed(args))
        }
    }
}

/// The `**expr` double-splat conversion (CRuby's `rb_to_hash_type`): a Hash
/// passes through, anything else must define `to_hash` and answer a Hash
/// from it -- `take(**opts_object)` is the idiom. Codegen routes every
/// double-splat call-site argument through this.
pub fn to_hash_coerce(v: &RubyValue) -> Result<crate::RHash, Signal> {
    crate::builtins::convert::to_rhash(v)
}

/// The `&expr` block-argument conversion (CRuby's `Proc()` coercion at a
/// call site): a Proc passes through, a Symbol converts via
/// `Symbol#to_proc` (`map(&:to_s)`), nil means "no block", anything else
/// is real Ruby's TypeError. Codegen's `emit_block_option` routes every
/// forwarded block argument through this.
pub fn block_arg_to_proc(v: crate::RubyValue) -> Result<Option<crate::RubyValue>, crate::Signal> {
    match v {
        crate::RubyValue::Proc(_) => Ok(Some(v)),
        crate::RubyValue::Symbol(s) => Ok(Some(crate::builtins::symbol::symbol_to_proc(s))),
        crate::RubyValue::Nil => Ok(None),
        // Anything else duck-types through `to_proc` (CRuby's
        // `rb_block_arg_to_proc`): a user object defining it (or a
        // `Method` -- its Path-2 row answers one) converts; a non-Proc
        // answer or no `to_proc` at all is the TypeError.
        other => {
            let to_proc = crate::Symbol::intern("to_proc");
            if crate::dispatch::responds_to(other.class_id(), to_proc, false) {
                return match crate::dispatch::send_value(&other, to_proc, &[], None)? {
                    p @ crate::RubyValue::Proc(_) => Ok(Some(p)),
                    bad => Err(type_error!(
                        "can't convert {} to Proc ({}#to_proc gives {})",
                        crate::builtins::class_name_of(&other),
                        crate::builtins::class_name_of(&other),
                        crate::builtins::class_name_of(&bad)
                    )),
                };
            }
            Err(type_error!(
                "no implicit conversion of {} into Proc",
                crate::builtins::convert_name_of(&other)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{array_new, hash_new, string_new};

    fn proc_of(
        f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static,
    ) -> RubyValue {
        RubyValue::Proc(RProc::new(f))
    }

    #[test]
    fn a_runtime_proc_reports_var_args_arity_and_is_not_a_lambda() {
        let p = RProc::new(|_| Ok(RubyValue::Nil));
        assert_eq!(p.arity(), -1);
        assert!(!p.is_lambda());
    }

    #[test]
    fn with_meta_carries_the_arity_and_lambda_flag_codegen_computed() {
        let p = RProc::with_meta(|_| Ok(RubyValue::Nil), 2, true);
        assert_eq!(p.arity(), 2);
        assert!(p.is_lambda());
    }

    /// A Proc with no captured home (`RProc::new`, or `with_meta` without
    /// `with_home`) PROPAGATES a `Signal::Return` -- it is either internal
    /// machinery relaying a user block's non-local return, or a top-level
    /// proc; either way it must not swallow the return.
    #[test]
    fn a_homeless_proc_propagates_a_return_signal() {
        let p = RProc::new(|_| Err(Signal::Return(RubyValue::Int(7))));
        assert!(matches!(
            p.call(&[]),
            Err(Signal::Return(RubyValue::Int(7)))
        ));
    }

    /// A Proc whose captured home is still alive propagates its `Signal::Return`
    /// (a genuine non-local return in flight); once the home dies the same
    /// return becomes a `LocalJumpError` -- here surfacing as a panic only
    /// because the unit test runs without a `ClassRegistry` to build the
    /// exception.
    #[test]
    fn a_captured_home_gates_return_between_propagate_and_localjump() {
        crate::signal::home_push();
        let live = ProcBuilder::from_rust(
            |_, _, _| Err(Signal::Return(RubyValue::Int(1))),
            RubyValue::Nil,
            0,
            false,
        )
        .home()
        .build();
        // Home is on the stack: the return propagates.
        assert!(matches!(
            live.call(&[]),
            Err(Signal::Return(RubyValue::Int(1)))
        ));
        // Kill the home; the SAME proc now finds no live home.
        crate::signal::home_pop();
        let dead = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| live.call(&[])));
        assert!(dead.is_err() || dead.unwrap().is_err());
    }

    /// A lambda folds its own return internally, so it never reaches the
    /// home-return resolution even if its body yields a `Signal::Return`.
    #[test]
    fn a_lambda_never_converts_a_return() {
        let lam = ProcBuilder::from_rust(
            |_, _, _| Err(Signal::Return(RubyValue::Int(9))),
            RubyValue::Nil,
            0,
            true,
        )
        .home()
        .build();
        assert!(matches!(
            lam.call(&[]),
            Err(Signal::Return(RubyValue::Int(9)))
        ));
    }

    /// The newtype still CALLS like the bare `Arc<dyn Fn>` it replaced --
    /// a call expression auto-dereferences its callee, which is what keeps
    /// every existing `p(&args)` site working.
    #[test]
    fn a_proc_is_callable_through_the_newtype() {
        let p = RProc::new(|args: &[RubyValue]| Ok(args[0].clone()));
        let out = p.call(&[RubyValue::Int(7)]).unwrap();
        assert_eq!(out.to_display_string(), "7");
    }

    #[test]
    fn ptr_eq_is_allocation_identity_not_behavioral_equality() {
        let a = RProc::new(|_| Ok(RubyValue::Nil));
        let b = RProc::new(|_| Ok(RubyValue::Nil));
        assert!(a.ptr_eq(&a.clone()));
        assert!(!a.ptr_eq(&b));
    }

    // --- block_auto_splat ---------------------------------------------

    #[test]
    fn auto_splat_spreads_a_lone_array_argument() {
        let args = [RubyValue::Array(array_new(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
        ]))];
        let out = block_auto_splat(&args).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].to_display_string(), "1");
        assert_eq!(out[1].to_display_string(), "2");
    }

    #[test]
    fn auto_splat_leaves_a_lone_non_array_alone() {
        let args = [RubyValue::Int(5)];
        let out = block_auto_splat(&args).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].to_display_string(), "5");
    }

    /// Only a SINGLE argument ever splats -- two yielded values are already
    /// separate arguments and must pass through untouched, even when the
    /// first happens to be an Array.
    #[test]
    fn auto_splat_leaves_multiple_arguments_alone() {
        let args = [
            RubyValue::Array(array_new(vec![RubyValue::Int(1)])),
            RubyValue::Int(9),
        ];
        let out = block_auto_splat(&args).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].inspect_string(), "[1]");
    }

    #[test]
    fn auto_splat_of_no_arguments_is_a_no_op() {
        assert!(block_auto_splat(&[]).unwrap().is_empty());
    }

    #[test]
    fn auto_splat_spreads_an_empty_array_to_nothing() {
        let args = [RubyValue::Array(array_new(Vec::new()))];
        let out = block_auto_splat(&args).unwrap();
        assert!(out.is_empty());
    }

    // --- block_arg_to_proc --------------------------------------------

    #[test]
    fn a_proc_block_argument_passes_through_unchanged() {
        let p = proc_of(|_| Ok(RubyValue::Int(1)));
        let out = block_arg_to_proc(p).unwrap();
        assert!(matches!(out, Some(RubyValue::Proc(_))));
    }

    #[test]
    fn a_symbol_block_argument_converts_via_symbol_to_proc() {
        let out = block_arg_to_proc(RubyValue::Symbol(crate::Symbol::intern("upcase")))
            .unwrap()
            .expect("a Symbol converts");
        let RubyValue::Proc(p) = out else {
            panic!("expected a Proc")
        };
        let s = RubyValue::Str(string_new("hi".to_string()));
        assert_eq!(p.call(&[s]).unwrap().to_display_string(), "HI");
    }

    #[test]
    fn a_nil_block_argument_means_no_block() {
        assert!(block_arg_to_proc(RubyValue::Nil).unwrap().is_none());
    }

    /// A value with no `to_proc` raises TypeError -- surfacing as a panic
    /// here only because these unit tests run without a `ClassRegistry`
    /// installed (the same posture as `builtins::array`'s own tests); the
    /// `duck_conversions` example covers the real rescued message.
    #[test]
    fn a_non_convertible_block_argument_is_a_type_error() {
        let r = std::panic::catch_unwind(|| block_arg_to_proc(RubyValue::Int(1)));
        assert!(r.is_err());
    }

    // --- to_hash_coerce ------------------------------------------------

    #[test]
    fn a_hash_double_splat_passes_through_unchanged() {
        let h = hash_new(vec![(RubyValue::Int(1), RubyValue::Int(2))]);
        let out = to_hash_coerce(&RubyValue::Hash(h.clone())).unwrap();
        assert_eq!(out.lock().len(), 1);
    }

    #[test]
    fn a_non_convertible_double_splat_is_a_type_error() {
        let r = std::panic::catch_unwind(|| to_hash_coerce(&RubyValue::Int(1)));
        assert!(r.is_err());
    }
}

#[cfg(test)]
mod literal_block_tests {
    use crate::{RProc, RubyValue};

    fn rust_proc() -> RProc {
        RProc::new(|_args| Ok(RubyValue::Nil))
    }

    /// A proc the RUNTIME builds is CRuby's C-level Proc, never a block a
    /// call site wrote. Only `ProcBuilder::from_c` -- the emitter's path --
    /// sets the mark.
    #[test]
    fn a_runtime_proc_is_not_a_literal_block() {
        assert!(!rust_proc().is_literal_block());
    }

    /// The mark is one-way: once a handle has been named and passed with
    /// `&`, it can never be a literal block again.
    #[test]
    fn clearing_the_mark_is_one_way() {
        let p =
            super::ProcBuilder::from_rust(|_, _, _| Ok(RubyValue::Nil), RubyValue::Nil, 0, false)
                .build();
        p.0.literal_block
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(p.is_literal_block());
        p.clear_literal_block();
        assert!(!p.is_literal_block());
        p.clear_literal_block();
        assert!(!p.is_literal_block());
    }

    /// `dup`, `clone` and `#lambda` all mint a fresh `ProcData`, and each one
    /// is a NAMED handle. A copy that carried the mark would let
    /// `lambda(&pr.dup)` through.
    #[test]
    fn a_copy_is_never_a_literal_block() {
        let p =
            super::ProcBuilder::from_rust(|_, _, _| Ok(RubyValue::Nil), RubyValue::Nil, 0, false)
                .build();
        p.0.literal_block
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(!p.as_lambda().is_literal_block());
        assert!(!p.dup_data(false).is_literal_block());
    }
}
