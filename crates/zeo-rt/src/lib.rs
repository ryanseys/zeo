//! zeo-rt: the runtime library every zeo-generated program links against.

/// Internal maps (interner, dispatch registry, globals/constants): foldhash
/// instead of SipHash -- process-internal keys need speed, not DoS
/// resistance. Ruby-visible `Object#hash` keeps `DefaultHasher`.
pub(crate) type FMap<K, V> = std::collections::HashMap<K, V, foldhash::fast::RandomState>;
pub(crate) type FSet<T> = std::collections::HashSet<T, foldhash::fast::RandomState>;
/// A two-level `box -> name -> value` map: the OUTER key is a box id
/// (`0` = main). `Box<str>` inner keys so every read probes with its
/// borrowed `&str`. Only the globals family is box-keyed today; Track 7
/// widens the set.
pub(crate) type BoxScopedMap<V> = FMap<u32, FMap<Box<str>, V>>;
/// A two-level `owner class -> name -> value` map: the OUTER key is the
/// owning `ClassId`'s raw `u32` -- NOT a box id, which the old shared
/// `ScopedMap` alias let readers confuse. Constants/cvars/civars key this
/// way and are deliberately box-blind (the documented divergence).
pub(crate) type ClassScopedMap<V> = FMap<u32, FMap<Box<str>, V>>;

mod arith;
mod bootstrap;
pub mod boxes;
mod builtins;
pub mod capi;
mod catch;
#[cfg(feature = "cext")]
pub mod cext;
mod civars;
pub mod compiled_object;

mod constants;
mod cvars;
mod dispatch;
mod ec;
mod enc;
pub mod encoding;
// Runtime string `eval`, and the seam the compiler is installed through.
mod coroutine;
pub mod eval;
mod exec;
mod ext;
pub mod features;
pub mod ffi;
/// Re-exported for GENERATED code, which speaks only `zeo_rt`: an FFI
/// `PlatformScalar` argument spells the target's own `zeo_rt::libc::mode_t`
/// so rustc supplies the real width where the program builds.
pub use libc;
mod fiber;
mod flipflop;
mod frames;
pub mod gc;
mod globals;
pub mod gvl;
mod handling;

mod lastmatch;
pub mod log;
mod method_meta;
mod mt;
pub mod pools;
mod ractor;
mod regexp;
mod release_pool;
mod rproc;
mod runtime_meta;
mod signal;
mod stack_guard;
mod symbol;
mod thread;
#[macro_use]
mod trace;
mod tramp;
mod value;
pub(crate) use value::{collections, ivars, value_ivars};

pub use arith::*;
pub use bootstrap::{install_core_constants, install_data_section, register_builtins};
pub use builtins::BuiltinMethodFn;
pub use builtins::binding::{LocalCell, RBinding, binding_new};
pub use builtins::complex::{RComplex, RComplexData, complex_from_literal, complex_new};
pub use builtins::enumerable::{ForBind, each_values};
pub use builtins::enumerator::{EnumeratorData, REnumerator};
pub use builtins::exception::{
    apply_custom_backtrace, attach_backtrace, backtrace_lines, pattern_fail_case_eq,
    pattern_fail_deconstruct, pattern_fail_find, pattern_fail_guard, pattern_fail_length,
    pattern_fail_not_empty, pattern_key_miss_clear, pattern_key_miss_record, pattern_match_error,
    pattern_match_error_bare, register_exception_subclass, register_exceptions, report_uncaught,
    set_backtrace_lines, set_explicit_cause,
};
pub use builtins::format::sprintf;
pub use builtins::kernel::{
    kernel_abort, kernel_array, kernel_complex, kernel_exit, kernel_exit_bang, kernel_float,
    kernel_format, kernel_hash, kernel_integer, kernel_p, kernel_pp, kernel_print, kernel_printf,
    kernel_puts, kernel_rand, kernel_rational, kernel_sleep, kernel_srand, kernel_string,
    kernel_throw, kernel_warn, system_exit_status,
};
pub use builtins::method::method_capture_inherited;
pub use builtins::rational::{RRational, RRationalData, rational_from_digits, rational_new};
pub use builtins::rstruct::register_compiled_struct;
pub use builtins::value_subclass::{
    register_recv_honouring_subclass, register_value_subclass, value_super,
};
pub use builtins::warning::emit_parse_warnings;
pub use builtins::weak::register_weakmap_subclass;
pub use builtins::weak::run_finalizers;
pub use catch::kernel_catch;
pub use civars::{CivarSite, class_ivar_get, class_ivar_names, class_ivar_set};
pub use collections::*;
pub use constants::{
    ConstSite, class_revealed, conceal_class, conditional_class_ref, const_get, const_get_master,
    const_get_scoped, const_is_private, const_set, const_set_at, const_set_private,
    const_set_private_value, record_const_location, reveal_class,
};
pub use cvars::{cvar_defined, cvar_get, cvar_get_checked, cvar_names_of, cvar_remove, cvar_set};
pub use dispatch::{
    ARRAY_CLASS, AllocatorFn, BASIC_OBJECT_CLASS, CLASS_CLASS, COMPARABLE_CLASS, COMPLEX_CLASS,
    CallSite, ClassId, ClassMethodSite, ClassRegistry, ConstructorFn, DynCallerSite,
    ENUMERABLE_CLASS, ENUMERATOR_CLASS, FALSE_CLASS, FCALL, FIBER_CLASS, FLOAT_CLASS, HASH_CLASS,
    INTEGER_CLASS, KERNEL_CLASS, MATCH_DATA_CLASS, MATH_CLASS, MODULE_CLASS, MUTEX_CLASS, MethodFn,
    MethodVisibility, MissingReason, NIL_CLASS, NUMERIC_CLASS, Object, PROC_CLASS, QUEUE_CLASS,
    RACTOR_CLASS, RANGE_CLASS, RATIONAL_CLASS, REGEXP_CLASS, RObj, Reflect, RubyObject,
    STRING_CLASS, STRUCT_CLASS, SYMBOL_CLASS, THREAD_CLASS, TRUE_CLASS, ValueMethodFn,
    YIELDER_CLASS, alias_in_default_definee, arity_error, bind_dynamic_kwargs,
    call_singleton_super_target, check_not_moved_obj, class_is_module, class_name,
    coerce_raise_arg, coerce_raise_arg_with_message, const_miss, construct_by_class_id,
    define_in_default_definee, describe_receiver, downcast_robj, downcast_robj_ref,
    guard_class_reopen, guard_public_class_method, install_class_registry, instance_variable_get,
    instance_variable_set, instance_variables, is_a, is_a_value, ivar_defined, ivar_frozen_error,
    ivar_get_dyn, ivar_get_dyn_isolated, ivar_name_arg, ivar_set_dyn, ivar_slot_get_dyn,
    ivar_slot_get_dyn_isolated, ivar_slot_set_dyn, main_object, make_name_error,
    method_name_symbol, raise_error, raise_error_details, raise_method_missing,
    raise_no_block_yield, raise_stop_iteration, raise_with_cause, refined_method,
    refined_responds_to, refined_send_dynamic, refined_send_in, reflect_dispatch_in,
    reject_marked_kwargs, rescue_matches_any, responds_to, responds_to_or_missing,
    responds_to_value, run_initialize, send, send_class_cached, send_in, send_super_class_from,
    send_super_from, send_value, send_value_cached, send_value_dyn_cached, send_value_explicit_in,
    send_value_in, send_value_public_in, send_value_vcall_in, stamp_backtrace, super_defined,
    validate_aliases, validate_class_aliases, value_class, wrong_arity,
};
pub use encoding::{EncodingId, StrBuf};
pub use eval::{eval_string, eval_value, eval_value_in_scope};
pub use exec::{at_exit_register, run_at_exit, run_main};
/// The compiled-prologue `TracePoint#self` note (`ext::tracepoint`) -- boxes
/// the receiver only while a trace hook is armed; free (and a no-op) in
/// builds without the tracepoint ext.
#[cfg(feature = "ext-tracepoint")]
pub use ext::tracepoint::trace_frame_self;
pub use flipflop::{flip_flop_on, flip_flop_set};
pub use gvl::mark_sole_thread;
pub use ivars::IvarCell;
pub use lastmatch::{
    last_match, last_match_group, last_match_last_group, last_match_post, last_match_pre,
    set_last_match, svar_scope,
};
pub use runtime_meta::{register_module_subclass, runtime_undef_class_method_names};
#[cfg(not(feature = "ext-tracepoint"))]
#[inline]
pub fn trace_frame_self(_f: impl FnOnce() -> RubyValue) {}
pub use method_meta::{MetaRow, MethodKind, MethodMeta, ParamKind, register_meta_rows};
/// The Ruby class name of any value -- what the generated top level suffixes an
/// uncaught exception's message with (`"msg (ClassName)"`, CRuby's own form).
pub fn class_name_of_value(v: &RubyValue) -> String {
    builtins::class_name_of(v)
}

pub use builtins::array::{array_pop_checked, array_push_checked, array_shift_checked};
pub use builtins::enumerable::SumAcc;
pub use builtins::process::last_child_status;
pub use builtins::rmodule::{
    const_defined_in, const_miss_signal, scope_const_defined, scope_const_get,
    scope_const_get_or_nil, scope_const_set,
};
#[cfg(feature = "ext-coverage")]
pub use ext::coverage::{cov_file_loaded, cov_line, coverage_install};
pub use fiber::{
    FiberHandle, FiberResume, FiberYield, RFiber, fiber_alive, fiber_current, fiber_new,
    fiber_raise, fiber_resume, fiber_transfer, fiber_yield,
};
pub use frames::{FrameGuard, caller_lines, capture_backtrace, set_line, synthetic_c_frame};
pub use globals::{
    global_alias, global_assign, global_defined, global_get, global_set, seed_load_path,
    seed_loaded_features,
};
pub use handling::{PropagatingGuard, current_exception, pop_handling, push_handling};
pub use pools::{LitPool, SymPool};
pub use ractor::{
    RRactor, RactorData, cross_boundary, current_ractor, in_main_ractor, init_main_ractor,
    ivar_isolation_check, make_shareable, make_shareable_value, moved_object_error, ractor_join,
    ractor_new, ractor_outcome, ractor_receive, ractor_select, ractor_send, ractor_send_mode,
    ractor_value, shareable,
};

/// `Ractor::MovedObject`'s numeric class id, spelled where `ruby_class!`'s
/// generated `retag_moved` can reach it through `$crate`.
pub const MOVED_OBJECT_CLASS_ID: u32 = zeo_abi::RACTOR_MOVED_OBJECT_CLASS.0;

pub use builtins::range::{RRange, RangeData, range_new, range_value};
pub use regexp::*;
pub use rproc::{ProcParamMeta, RProc, block_arg_to_proc, block_auto_splat, to_hash_coerce};
pub use runtime_meta::{
    class_maybe_patched, copy_value_singletons, fire_const_added, iter_inline_ok,
    iter_inline_ok_for, mark_global_def_hook, name_runtime_class_if_anonymous,
    register_singleton_surrogate, runtime_class_method_visibility, runtime_class_new,
    runtime_define_method, runtime_define_singleton_method, runtime_replace_method,
    runtime_set_visibility, send_super_dynamic, value_extends, with_pending_defs,
};
pub use signal::{Signal, catch_break, home_pop, home_push, return_targets_here};
pub use stack_guard::stack_check;
pub use symbol::Symbol;
pub use thread::{
    MutexData, QueueData, RMutex, RQueue, RThread, ThreadData, mutex_lock, mutex_locked, mutex_new,
    mutex_owned, mutex_try_lock, mutex_unlock, queue_close, queue_closed, queue_is_sized,
    queue_len, queue_max, queue_new, queue_pop, queue_push, queue_set_max, sized_queue_new,
    thread_new, thread_outcome,
};
pub use value::rb_eq_checked;
pub use value::{RubyValue, observed_class_id};
pub use value::{case_eq, case_eq_any};
pub use value::{range_checked, range_endpoint, range_endpoint_error};
/// Re-exported so generated programs can name it (`regexp_new_enc`'s last
/// argument) without depending on `zeo-abi` themselves.
pub use zeo_abi::RegexpEncoding;

/// The interruption checkpoint generated code plants at loop back-edges and
/// method prologues: ONE relaxed load of the process-wide pending counter
/// (see `gvl`), and only a nonzero value takes the slow path that actually
/// resolves the current thread and delivers a queued `Thread#kill`/`#raise`.
/// This is what makes a busy loop killable -- with or without a GVL.
#[inline]
pub fn check_ints() -> Result<(), Signal> {
    if gvl::interrupts_pending_anywhere() {
        // Quantum tick first (a yield under an armed GVL, a no-op consume
        // otherwise), then any queued kill/raise for THIS thread.
        gvl::service_timer();
        // Before the thread's own interrupts: a kill delivered here would
        // unwind past a collector that is waiting for this thread to park.
        gvl::park_if_asked();
        // After parking, so a thread the collector asked to stop does not
        // start a second collection of its own.
        gc::service_due();
        thread::emitted_checkpoint()?;
    }
    Ok(())
}

/// The checkpoint a BLOCKING primitive runs before it parks: the same
/// timer/collector servicing as [`check_ints`] -- a sleeper must
/// acknowledge a collector's park request before going quiet -- but with
/// RAW interrupt delivery. A park entry is a delivery point in CRuby even
/// for a spawned thread's first checkpoint, so the `body_entered` one-shot
/// does not apply here (`Thread.new { sleep }` + an immediate `#raise`
/// must wake now, not park forever).
#[inline]
pub fn blocking_checkpoint() -> Result<(), Signal> {
    if gvl::interrupts_pending_anywhere() {
        gvl::service_timer();
        gvl::park_if_asked();
        gc::service_due();
        thread::check_interrupt()?;
    }
    Ok(())
}

// libc's `mode_t`, `dev_t`, `blksize_t` and `suseconds_t` are narrower on
// macOS than on Linux, and `c_char` is signed on macOS and x86-64 but not on
// aarch64 Linux, so a bare `as` is a conversion on one platform and an
// "unnecessary cast" clippy error on the other. These convert on both.
#[allow(clippy::unnecessary_cast)]
pub(crate) fn c_char_u8(c: libc::c_char) -> u8 {
    c as u8
}
#[allow(clippy::unnecessary_cast)]
pub(crate) fn mode_u32(mode: libc::mode_t) -> u32 {
    mode as u32
}
#[allow(clippy::unnecessary_cast)]
pub(crate) fn dev_u64(dev: libc::dev_t) -> u64 {
    dev as u64
}
#[allow(clippy::unnecessary_cast)]
pub(crate) fn blksize_i64(blksize: libc::blksize_t) -> i64 {
    blksize as i64
}
#[allow(clippy::unnecessary_cast)]
pub(crate) fn usec_i64(usec: libc::suseconds_t) -> i64 {
    usec as i64
}

/// The C `errno` slot for this thread (the accessor's name is per-libc).
pub(crate) fn errno_ptr() -> *mut libc::c_int {
    #[cfg(target_vendor = "apple")]
    unsafe {
        libc::__error()
    }
    #[cfg(not(target_vendor = "apple"))]
    unsafe {
        libc::__errno_location()
    }
}

/// Re-exported so `ruby_class!`'s macro-expanded code (which runs inside a
/// GENERATED program's own crate, not this one) can reference
/// `$crate::parking_lot::Mutex` without that program's own `Cargo.toml`
/// needing a direct `parking_lot` dependency -- `parking_lot` stays an
/// implementation detail of this runtime crate.
pub use parking_lot;

/// Mirrors CRuby's `Kernel#puts` for the single scalar-argument case (the
/// only form the original examples used): print the value, adding a trailing
/// newline only if it doesn't already end in one.
pub fn puts(value: RubyValue) {
    let s = value.to_display_string();
    if s.ends_with('\n') {
        print!("{s}");
    } else {
        println!("{s}");
    }
}

/// `Kernel#p`, single-argument form: prints the INSPECT
/// rendering (`p [1, "x"]` -> `[1, "x"]`, `p Widget` -> `Widget`) and
/// returns its argument (real Ruby's contract; `puts` returns nil).
/// Multi-argument/zero-argument forms are separate breadth.
pub fn p(value: RubyValue) -> RubyValue {
    println!("{}", value.inspect_string());
    value
}

/// One declarative macro absorbs the struct/trait-impl/registration ceremony
/// zeo's codegen hand-emits as raw C text per class (`emit_class_struct`,
/// `emit_class_new`, etc.) -- see the plan's "The `ruby_class!` macro"
/// section for the full rationale. Each ivar becomes an individually
/// interior-mutable field (`parking_lot::Mutex<RubyValue>`, not `RefCell`)
/// so every generated method can uniformly take a receiver, and so every
/// generated struct is genuinely `Send + Sync`.
///
/// Every method receiver is `self: Arc<Self>`, not `&self` -- escaping
/// closures need this: a real escaping `Proc` closure that references `self`/an ivar has
/// to capture an OWNED, `'static` handle (a `RubyValue`-stored `Proc` has no
/// lifetime parameter anywhere in this codebase), which a borrowed `&self`
/// can never provide. Every call site already hands over an `Arc<Concrete>`
/// (`New`'s result, and every local/ivar holding an object, are `Arc`-wrapped
/// from the moment they're built -- see `new_handle`'s docs below), so this
/// costs nothing at ordinary call sites; it only removes a capability
/// (capturing `self` into a closure) that didn't exist before.
///
/// `$slf` (not a hardcoded `self`) is still captured from the caller's own
/// tokens, exactly as before the `Rc<Self>` -> `Arc<Self>` migration --
/// confirmed the hard way: a `self` written directly in THIS template is
/// hygienically distinct from a `self` written inside the caller's `$body`,
/// so `rustc` rejects it (`E0424`, "self value is a keyword only available
/// in methods with a self parameter") the moment `$body` references
/// `self.some_ivar`. Only the TYPE annotation is now fixed to `Arc<Self>`
/// (rather than varying, since `&self` never varied either -- `$slf:tt`
/// only ever captured the single token `self`, never its type).
///
/// `ancestors` is the full, real linearized MRO (this class first, then
/// prepends/includes/superclass in resolution order -- see
/// `zeo::analyze::mro::compute_ancestors`), computed entirely at
/// zeo compile time and baked in here as a literal list; `__register`
/// just forwards it. No separate `ruby_module!` macro exists: a Ruby
/// `module`'s methods are never emitted as their own Rust struct/impl at
/// all -- they're MATERIALIZED directly onto whichever class(es)
/// include/prepend/extend them, so every method
/// this macro ever sees already belongs, concretely, to `$name`.
/// Turns one ivar name into `()`, so `ruby_class!` can count its ivars with
/// `<[()]>::len` -- a const expression, which is what the `IvarCell<N>` field
/// type needs. `macro_rules!` has no stable way to count a repetition.
#[macro_export]
#[doc(hidden)]
macro_rules! ivar_unit {
    ($ivar:ident) => {
        ()
    };
}

#[macro_export]
macro_rules! ruby_class {
    (
        class $name:ident : $super:path {
            id: $id:expr_2021;
            name: $rname:literal;
            ancestors: [ $($anc:expr_2021),* $(,)? ];
            ivars { $($ivar:ident),* $(,)? }
            hidden { $($hidden:ident),* $(,)? }
            $( def $method:ident ( $slf:tt : std::sync::Arc<Self> $(, $arg:ident : $arg_ty:ty)* $(,)? ) $body:block )*
            dispatch { $( $dname:literal => $tramp:expr_2021 ),* $(,)? }
        }
    ) => {
        // A caller may pass a mangled, deliberately non-CamelCase Rust
        // name for a nested class; a top-level class's plain name
        // already is CamelCase.
        #[allow(non_camel_case_types)]
        pub struct $name {
            /// `.freeze`'s per-object flag -- read through
            /// `RubyObject::is_frozen` and by the guard the emitter places
            /// before every ivar write. Double-underscore
            /// prefixed, matching the `__blk`/`__self` convention for
            /// generated names a user ivar won't realistically collide with
            /// (a literal Ruby `@__frozen` would -- same accepted, vanishing
            /// residual risk as every other `__`-reserved name in codegen).
            pub __frozen: std::sync::atomic::AtomicBool,
            /// Every ivar this class names, in declaration order, behind ONE
            /// lock -- plus whatever a runtime path invents. See
            /// [`IvarCell`](crate::IvarCell): the slot INDEX is what generated
            /// code uses, and `__IVAR_NAMES` is what the by-name reflection
            /// paths resolve against.
            /// The declared ivars come first, then the HIDDEN slots -- a
            /// `Struct`/`Data` member, which is real storage that is not an
            /// instance variable. Keeping them in the same cell means a member
            /// costs a member exactly what an ivar costs, and keeping them
            /// AFTER means every by-name path, which walks `__IVAR_NAMES`,
            /// stops before reaching one.
            pub __ivars: $crate::IvarCell<{
                <[()]>::len(&[$($crate::ivar_unit!($ivar),)* $($crate::ivar_unit!($hidden),)*])
            }>,
            /// The instance's ACTUAL class id -- `Self::CLASS_ID` for every
            /// statically-constructed instance, and the runtime subclass's id
            /// when `Class.new(CompiledBase)` allocates through this struct
            /// (see `runtime_meta::compiled_subclass_construct`). One
            /// generated struct type can back many class ids, the same
            /// precedent `ValueSubclass`/`RubyException` set -- which is what
            /// lets an inherited compiled method's trampoline downcast
            /// successfully instead of aborting. Written at construction,
            /// atomic (relaxed -- a plain load on every real target) for the
            /// ONE later write that exists: a `Ractor` move's retag to
            /// `Ractor::MovedObject`, which is what makes a husk's
            /// `class_id()` -- and with it dispatch and reflection -- answer
            /// the husk class with no extra probe anywhere.
            pub __class: std::sync::atomic::AtomicU32,
        }

        impl $name {
            pub const CLASS_ID: $crate::ClassId = $crate::ClassId($id);

            /// The declared ivar names, minus the `@`, positionally matching
            /// the cell's slots. The field idents ARE the names (codegen's
            /// `safe_ident`), so `stringify!` recovers them with no second
            /// list to keep in sync. Only the by-NAME paths -- reflection, and
            /// a receiver whose class codegen could not know -- consult it;
            /// generated reads and writes carry the index.
            pub const __IVAR_NAMES: &'static [&'static str] = &[$(stringify!($ivar)),*];

            /// Where the NAMED slots start -- the hidden `Struct`/`Data`
            /// members come first, so a subclass and its struct ancestor put
            /// a member at the same index (see `ClassLayout::hidden`).
            pub const __IVAR_BASE: usize = Self::__HIDDEN_COUNT;

            /// How many `Struct`/`Data` members this class declares; `0` for
            /// every ordinary class.
            pub const __HIDDEN_COUNT: usize =
                <[()]>::len(&[$($crate::ivar_unit!($hidden)),*]);

            /// Accepts an already-`Arc`-wrapped value (never `Self` by
            /// value): generated code stores every `New`-constructed object
            /// as `Arc<ConcreteStruct>` from the moment it's
            /// built, so that a local variable holding
            /// it can be read more than once via a cheap, identity-preserving
            /// `Arc::clone()` rather than needing (and not having) a `Clone`
            /// impl on the bare struct itself -- deriving one naively would
            /// deep-copy each `Mutex` ivar, breaking Ruby's shared-mutable-
            /// object-identity semantics (`b = Box.new(1); c = b` must alias,
            /// not duplicate). This just returns `inner` unchanged, relying on
            /// `Arc<Concrete> -> Arc<dyn RubyObject>` unsized coercion at the
            /// return site to produce `RObj`. `Arc`, not `Rc`: every
            /// generated struct is genuinely `Send + Sync` once its fields
            /// are `Mutex`-wrapped, needed for `Thread`/`Ractor` to ever cross
            /// a real OS thread boundary -- no `unsafe impl` involved, this
            /// falls out of ordinary auto-trait derivation.
            pub fn new_handle(inner: std::sync::Arc<Self>) -> $crate::RObj {
                inner
            }

            /// The dynamic constructor -- registered into the
            /// `ClassRegistry` so `x = Widget; x.new(...)` (a class known
            /// only at runtime as a `RubyValue::Class`) can allocate a
            /// fresh instance and run `initialize` through the SAME
            /// dispatch trampoline an explicit `send(:initialize)` would
            /// use (see `$crate::run_initialize` for the no-initialize
            /// rule). Static `Widget.new(...)` call sites never come here.
            pub fn __construct(
                // A generated class has its own Rust type, so it ignores the
                // id the `ConstructorFn` contract passes and uses its own
                // `Self::CLASS_ID`. The parameter exists so ONE native
                // constructor can back many classes (the built-in exceptions);
                // see `ConstructorFn`.
                _class: $crate::ClassId,
                args: &[$crate::RubyValue],
                block: Option<$crate::RubyValue>,
            ) -> Result<$crate::RubyValue, $crate::Signal> {
                let handle = Self::__allocate(Self::CLASS_ID);
                $crate::run_initialize(Self::CLASS_ID, &handle, args, block)?;
                Ok($crate::RubyValue::Object(handle))
            }

            /// The no-`initialize` allocator backing `Class#allocate` --
            /// registered as this class's `AllocatorFn`. Builds the same
            /// zero-initialized struct `__construct` does (every ivar `Nil`,
            /// unfrozen) but stops there, so the caller gets a bare instance
            /// to populate by hand. The PASSED id is stored as the instance's
            /// class: `Self::CLASS_ID` from every static path, the runtime
            /// subclass's id when `Class.new(CompiledBase)` allocates its
            /// instances through the compiled ancestor's struct.
            pub fn __allocate(class: $crate::ClassId) -> $crate::RObj {
                let o: $crate::RObj = std::sync::Arc::new($name {
                    __frozen: std::sync::atomic::AtomicBool::new(false),
                    __ivars: $crate::IvarCell::new(),
                    __class: std::sync::atomic::AtomicU32::new(class.0),
                });
                $crate::gc::record_object(&o);
                o
            }

            $(
                // Return type is always `Result<RubyValue, Signal>`, never
                // omitted or bare `RubyValue`: Ruby methods always implicitly
                // return a value (the last expression, or nil) -- codegen
                // emits `Ok(RubyValue::Nil)` for methods with nothing
                // meaningful to return (e.g. `initialize`) -- and every
                // method body is a `?`-propagation boundary for non-local
                // control flow from the first breadth phase onward, not just
                // once `raise`/escaping `Proc`s exist (see `signal.rs`).
                pub fn $method($slf: std::sync::Arc<Self> $(, $arg: $arg_ty)*) -> Result<$crate::RubyValue, $crate::Signal> $body
            )*
        }

        impl $crate::RubyObject for $name {
            fn class_id(&self) -> $crate::ClassId {
                $crate::ClassId(self.__class.load(std::sync::atomic::Ordering::Relaxed))
            }
            fn retag_moved(&self) -> bool {
                self.__class.store(
                    $crate::MOVED_OBJECT_CLASS_ID,
                    std::sync::atomic::Ordering::Relaxed,
                );
                true
            }
            fn as_any(&self) -> &dyn std::any::Any { self }
            fn as_any_rc(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync> { self }
            fn is_frozen(&self) -> bool { self.__frozen.load(std::sync::atomic::Ordering::Relaxed) }
            fn set_frozen(&self) { self.__frozen.store(true, std::sync::atomic::Ordering::Relaxed) }
            // Only ASSIGNED ivars, here and in `ivar_pairs`: a declared slot
            // nothing ever wrote does not exist as far as Ruby is concerned.
            fn ivar_values(&self) -> Vec<$crate::RubyValue> {
                self.__ivars.values(Self::__IVAR_BASE, Self::__IVAR_NAMES.len())
            }
            // Field-declaration order, `@`-prefixed to match Ruby's ivar
            // names -- the field idents ARE the names minus the `@` (see
            // `ivar_get_named`'s `stringify!` note), so no extra list to keep
            // in sync.
            fn ivar_pairs(&self) -> Vec<(String, $crate::RubyValue)> {
                self.__ivars.pairs(Self::__IVAR_BASE, Self::__IVAR_NAMES)
            }
            // By-NAME ivar access, for receivers whose concrete class codegen
            // couldn't know statically (`instance_exec`'s rebound self). The
            // field idents ARE the ivar names minus the `@` (codegen's
            // `safe_ident`), so `stringify!` recovers them with no extra list
            // to keep in sync. See the trait's docs for the invented-ivar TODO.
            fn ivar_get_named(&self, name: &str) -> Option<$crate::RubyValue> {
                self.__ivars.get_named(Self::__IVAR_BASE, Self::__IVAR_NAMES, name)
            }
            fn ivar_set_named(&self, name: &str, v: $crate::RubyValue) -> bool {
                self.__ivars.set_named(Self::__IVAR_BASE, Self::__IVAR_NAMES, name, v);
                true
            }
            // `Kernel#remove_instance_variable` -- empties the slot and returns
            // the old value, or `None` for a name that was never assigned (the
            // caller raises `NameError`), which is now a real distinction
            // rather than the approximation the old always-present slot forced.
            fn ivar_remove_named(&self, name: &str) -> Option<$crate::RubyValue> {
                self.__ivars.remove_named(Self::__IVAR_BASE, Self::__IVAR_NAMES, name)
            }
            // A `Struct`/`Data` MEMBER by position -- see the trait method's
            // docs. Offset past the real ivars, which is what keeps a member
            // out of every by-name path while costing it nothing to reach.
            fn hidden_ivar_get(&self, i: usize) -> Option<$crate::RubyValue> {
                (i < Self::__HIDDEN_COUNT).then(|| self.__ivars.get(i))
            }
            fn hidden_ivar_set(&self, i: usize, v: $crate::RubyValue) -> bool {
                let ok = i < Self::__HIDDEN_COUNT;
                if ok { self.__ivars.set(i, v); }
                ok
            }
            // By-SLOT ivar access, for a body shared across a hierarchy: the
            // receiver is a `RubyValue` there, but the index is still a
            // compile-time constant. See the trait method's docs.
            // Bounded by the WHOLE cell, not just the named region: a shared
            // body's slot index spans the hidden slots too, because a compiled
            // `Struct` and a subclass of it agree on where a member lives even
            // though one calls it a member and the other an ivar.
            fn ivar_slot_get(&self, slot: usize) -> $crate::RubyValue {
                if slot < Self::__IVAR_BASE + Self::__IVAR_NAMES.len() {
                    self.__ivars.get(slot)
                } else {
                    $crate::RubyValue::Nil
                }
            }
            fn ivar_slot_set(&self, slot: usize, value: $crate::RubyValue) {
                if slot < Self::__IVAR_BASE + Self::__IVAR_NAMES.len() {
                    self.__ivars.set(slot, value);
                }
            }
            fn gc_visit(&self, out: &mut Vec<$crate::RubyValue>, take: bool) {
                self.__ivars.gc_visit(out, take);
            }

            fn take_linked_ivars(&self, out: &mut Vec<$crate::RubyValue>) {
                self.__ivars.take_linked(out);
            }
            // `Kernel#dup`/`#clone`'s shallow copy (see the trait method's
            // docs): fresh struct, each ivar's CURRENT value cloned (a
            // handle clone -- nested objects stay shared), frozen flag
            // carried over only for `clone` (`copy_frozen`).
            fn dup_object(&self, copy_frozen: bool) -> $crate::RObj {
                let copy: $crate::RObj = std::sync::Arc::new($name {
                    __frozen: std::sync::atomic::AtomicBool::new(
                        copy_frozen && $crate::RubyObject::is_frozen(self),
                    ),
                    __ivars: self.__ivars.duplicate(),
                    // A dup of a runtime-subclass instance stays that class.
                    __class: std::sync::atomic::AtomicU32::new(
                        self.__class.load(std::sync::atomic::Ordering::Relaxed),
                    ),
                });
                $crate::gc::record_object(&copy);
                copy
            }
        }

        impl $name {
            /// Called once from generated `main()`. Populates the *runtime*
            /// dispatch table (Path 2) with a trampoline per method; Path 1
            /// (static) call sites never go through this table at all.
            ///
            /// Each trampoline's BODY is supplied by the caller (`$tramp`,
            /// via the `dispatch { ... }` block below) rather than derived
            /// here from `$arg`/`$arg_ty` pairs: once a method's params can
            /// be optional/rest/keyword instead of uniformly required, a
            /// bare exact-length slice-pattern match (the ONLY thing this
            /// macro could derive on its own from the `def` clauses above)
            /// cannot express the binding logic -- zeo already
            /// has the full, precise parameter-kind info to author it
            /// correctly (`clif::params::define_trampoline`), so this macro's job
            /// shrinks to exactly what its own doc comment always claimed:
            /// struct/impl/registration ceremony, not binding logic.
            ///
            /// `$dname` is a STRING LITERAL of the method's real Ruby name
            /// (`"tag="`, `"empty?"`, ...), not the escaped Rust identifier
            /// used for the `def` clause above (`tag_set`, `empty_p`, ...) --
            /// confirmed the hard way this distinction matters: an earlier
            /// version used `$dname:ident` + `stringify!($dname)` here, which
            /// registered every escaped-name method under its ESCAPED name
            /// instead of its real one, silently breaking `send`/`rescue`'s
            /// own re-raise-then-`.send(:tag=, ...)` for any method whose
            /// name needed escaping at all.
            pub fn __register(registry: &mut $crate::ClassRegistry) {
                registry.register(
                    Self::CLASS_ID,
                    $rname,
                    false,
                    vec![$($crate::ClassId($anc)),*],
                    Some(Self::__construct as $crate::ConstructorFn),
                );
                registry.define_allocator(
                    Self::CLASS_ID,
                    Self::__allocate as $crate::AllocatorFn,
                );
                $(
                    registry.define_method(
                        Self::CLASS_ID,
                        $crate::Symbol::intern($dname),
                        $tramp,
                    );
                )*
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    ruby_class! {
        class Point : Object {
            // Test ids sit FAR above the builtin range -- a builtin id here
            // (100 was zlib's `Deflate` once ids grew past it) makes the
            // registry answer the real table and every downcast below panic.
            id: 9100;
            name: "Point";
            ancestors: [9100, 0, 25, 24]; // [Point, Object, Kernel, BasicObject]
            ivars { x }
            hidden { }
            def initialize(self: std::sync::Arc<Self>, x: RubyValue) { self.__ivars.set(0, x); Ok(RubyValue::Nil) }
            def x(self: std::sync::Arc<Self>) { Ok(self.__ivars.get(0)) }
            dispatch {
                "initialize" => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Point>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [x] => Point::initialize(this, x.clone()),
                        _ => panic!("wrong number of arguments for initialize"),
                    }
                },
                "x" => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Point>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [] => Point::x(this),
                        _ => panic!("wrong number of arguments for x"),
                    }
                },
            }
        }
    }

    ruby_class! {
        class Greeter : Object {
            id: 9101;
            name: "Greeter";
            ancestors: [9101, 0, 25, 24];
            ivars { }
            hidden { }
            def hello(self: std::sync::Arc<Self>) { Ok(RubyValue::Str(string_new("hi".to_string()))) }
            def method_missing(self: std::sync::Arc<Self>, name: RubyValue) {
                Ok(RubyValue::Str(string_new(format!("no such method: {}", name.to_display_string()))))
            }
            dispatch {
                "hello" => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Greeter>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [] => Greeter::hello(this),
                        _ => panic!("wrong number of arguments for hello"),
                    }
                },
                "method_missing" => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Greeter>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [name] => Greeter::method_missing(this, name.clone()),
                        _ => panic!("wrong number of arguments for method_missing"),
                    }
                },
            }
        }
    }

    ruby_class! {
        class Temp : Object {
            id: 9102;
            name: "Temp";
            ancestors: [9102, 22, 0, 25, 24]; // [Temp, Comparable, Object, Kernel, BasicObject]
            ivars { deg }
            hidden { }
            def cmp(self: std::sync::Arc<Self>, other: RubyValue) {
                let mine = self.__ivars.get(0);
                let theirs = match &other {
                    RubyValue::Object(o) => {
                        let t = downcast_robj::<Temp>(o).expect("Temp <=> Temp only in tests");

                        t.__ivars.get(0)
                    }
                    _ => return Ok(RubyValue::Nil),
                };
                Ok(match mine.rb_cmp(&theirs) {
                    Some(o) => RubyValue::Int(o),
                    None => RubyValue::Nil,
                })
            }
            dispatch {
                "<=>" => |recv, args: &[RubyValue], _blk: Option<RubyValue>| {
                    let this = downcast_robj::<Temp>(recv).expect("class_id guarantees this downcast");
                    match args {
                        [other] => Temp::cmp(this, other.clone()),
                        _ => panic!("wrong number of arguments for <=>"),
                    }
                },
            }
        }
    }

    fn point_at(x: i64) -> std::sync::Arc<Point> {
        let p = std::sync::Arc::new(Point {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Point::CLASS_ID.0),
        });
        p.__ivars.set(0, RubyValue::Int(x));
        p
    }

    fn temp(deg: i64) -> RubyValue {
        let t = std::sync::Arc::new(Temp {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Temp::CLASS_ID.0),
        });
        t.__ivars.set(0, RubyValue::Int(deg));
        RubyValue::Object(Temp::new_handle(t))
    }

    /// The class registry is a process-wide `OnceLock`, rejecting a second
    /// install -- exactly the "install once, from `main()`, before anything
    /// else runs" contract a real generated program relies on. Rust's test
    /// harness runs each `#[test]` on its own OS thread, so `std::sync::Once`
    /// here makes the test helper match that one-time-installation contract.
    fn install() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let mut registry = ClassRegistry::new();
            registry.register(
                Object::CLASS_ID,
                "Object",
                false,
                vec![Object::CLASS_ID, KERNEL_CLASS, BASIC_OBJECT_CLASS],
                None,
            );
            registry.register(KERNEL_CLASS, "Kernel", true, vec![KERNEL_CLASS], None);
            registry.register(
                BASIC_OBJECT_CLASS,
                "BasicObject",
                false,
                vec![BASIC_OBJECT_CLASS],
                None,
            );
            Point::__register(&mut registry);
            Greeter::__register(&mut registry);
            Temp::__register(&mut registry);
            // Builtin entries + reopen VALUE METHODS for the
            // dispatch-precedence tests below. `Array`(5)/`Queue`(17) are
            // chosen because their ids don't collide with the test classes
            // above (Point/Greeter/Temp claimed 1-3, overlapping the low
            // builtin ids -- harmless until a builtin ENTRY is actually
            // registered), and the method names don't disturb anything
            // other tests in this process render or send (`Array#inspect`
            // stays native; only one test displays a Queue).
            registry.register(
                ARRAY_CLASS,
                "Array",
                false,
                vec![
                    ARRAY_CLASS,
                    ENUMERABLE_CLASS,
                    Object::CLASS_ID,
                    KERNEL_CLASS,
                    BASIC_OBJECT_CLASS,
                ],
                None,
            );
            registry.register(
                QUEUE_CLASS,
                "Thread::Queue",
                false,
                vec![
                    QUEUE_CLASS,
                    Object::CLASS_ID,
                    KERNEL_CLASS,
                    BASIC_OBJECT_CLASS,
                ],
                None,
            );
            registry.define_value_method(
                ARRAY_CLASS,
                0,
                Symbol::intern("shout"),
                |recv, _args, _blk| {
                    Ok(RubyValue::Str(string_new(format!(
                        "{}!",
                        recv.inspect_string()
                    ))))
                },
            );
            registry.define_value_method(
                ARRAY_CLASS,
                0,
                Symbol::intern("length"),
                |_recv, _args, _blk| Ok(RubyValue::Int(42)),
            );
            registry.define_value_method(
                QUEUE_CLASS,
                0,
                Symbol::intern("to_s"),
                |_recv, _args, _blk| {
                    Ok(RubyValue::Str(string_new(
                        "#<a queue, reopened>".to_string(),
                    )))
                },
            );
            install_class_registry(registry);
        });
    }

    #[test]
    fn static_path_stores_and_reads_ivar() {
        let p = std::sync::Arc::new(Point {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Point::CLASS_ID.0),
        });
        p.clone().initialize(RubyValue::Int(5)).unwrap();
        match p.x().unwrap() {
            RubyValue::Int(5) => {}
            other => panic!("expected Int(5), got {}", other.to_display_string()),
        }
    }

    #[test]
    fn new_handle_erases_to_a_trait_object() {
        let handle: RObj = Point::new_handle(point_at(7));
        assert_eq!(handle.class_id(), Point::CLASS_ID);
    }

    /// The freeze tiering `RubyValue::is_frozen`/`freeze_value`
    /// implement -- immediates/`Range` always frozen, mutable types start
    /// unfrozen and latch on `.freeze` (which returns self and no-ops when
    /// repeated); Queue is the one unfreezable kind (TypeError).
    #[test]
    fn freeze_tiering_matches_cruby_semantics() {
        assert!(RubyValue::Int(1).is_frozen());
        assert!(RubyValue::Nil.is_frozen());
        assert!(RubyValue::Symbol(Symbol::intern("s")).is_frozen());
        assert!(crate::builtins::range::range_value(None, None, false).is_frozen());

        let arr = RubyValue::Array(array_new(vec![RubyValue::Int(1)]));
        assert!(!arr.is_frozen());
        let same = arr.freeze_value().unwrap();
        assert!(arr.is_frozen());
        // Returns SELF (the same shared storage), not a copy.
        assert!(same.is_frozen());
        arr.freeze_value().unwrap(); // repeat freeze is a silent no-op
        assert!(arr.is_frozen());

        let s = RubyValue::Str(string_new("abc".to_string()));
        assert!(!s.is_frozen());
        s.freeze_value().unwrap();
        assert!(s.is_frozen());

        let obj = RubyValue::Object(Point::new_handle(std::sync::Arc::new(Point {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Point::CLASS_ID.0),
        })));
        assert!(!obj.is_frozen());
        obj.freeze_value().unwrap();
        assert!(obj.is_frozen());

        assert_eq!(
            arr.inspect_string(),
            "[1]",
            "inspect backs the FrozenError message format"
        );
    }

    #[test]
    fn dynamic_send_finds_registered_method() {
        install();
        let g: RObj = Greeter::new_handle(std::sync::Arc::new(Greeter {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Greeter::CLASS_ID.0),
        }));
        let result = send(&g, Symbol::intern("hello"), &[], None).unwrap();
        assert_eq!(result.to_display_string(), "hi");
    }

    #[test]
    fn dynamic_send_falls_back_to_method_missing() {
        install();
        let g: RObj = Greeter::new_handle(std::sync::Arc::new(Greeter {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Greeter::CLASS_ID.0),
        }));
        let result = send(&g, Symbol::intern("nope"), &[], None).unwrap();
        assert_eq!(result.to_display_string(), "no such method: nope");
    }

    /// `ruby_class!`-generated `dup_object` -- fresh instance,
    /// ivars copied by value-handle (mutating the copy's ivar leaves the
    /// original untouched), frozen flag copied only by `clone`.
    #[test]
    fn dup_object_copies_ivars_into_a_fresh_instance() {
        let p = point_at(1);

        let copy = RubyObject::dup_object(&*p, false);
        let concrete = downcast_robj::<Point>(&copy).expect("same concrete class");
        concrete.__ivars.set(0, RubyValue::Int(99));

        assert_eq!(p.__ivars.get(0).to_display_string(), "1");
        assert_eq!(concrete.__ivars.get(0).to_display_string(), "99");
    }

    #[test]
    fn dup_object_frozen_flag_follows_the_copy_frozen_rule() {
        let p = std::sync::Arc::new(Point {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Point::CLASS_ID.0),
        });
        RubyObject::set_frozen(&*p);

        assert!(
            !RubyObject::dup_object(&*p, false).is_frozen(),
            "dup: unfrozen"
        );
        assert!(
            RubyObject::dup_object(&*p, true).is_frozen(),
            "clone: frozen"
        );
    }

    /// Placement regression: universal `Kernel#dup` must be found BEFORE
    /// the `method_missing` fallback (real Ruby's lookup order) -- Greeter
    /// defines `method_missing`, which must NOT swallow `dup`.
    #[test]
    fn dynamic_send_dup_wins_over_method_missing() {
        install();
        let g: RObj = Greeter::new_handle(std::sync::Arc::new(Greeter {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Greeter::CLASS_ID.0),
        }));
        let result = send(&g, Symbol::intern("dup"), &[], None).unwrap();
        let RubyValue::Object(copy) = result else {
            panic!("dup must produce an Object, not a method_missing string")
        };
        assert_eq!(copy.class_id(), Greeter::CLASS_ID);
    }

    /// `send_value`'s universal `dup`/`clone` arm over a builtin receiver.
    #[test]
    fn send_value_dup_copies_a_builtin_value() {
        let arr = RubyValue::Array(array_new(vec![RubyValue::Int(1)]));
        let copy = send_value(&arr, Symbol::intern("dup"), &[], None).unwrap();
        send_value(&copy, Symbol::intern("push"), &[RubyValue::Int(2)], None).unwrap();

        assert_eq!(arr.inspect_string(), "[1]");
        assert_eq!(copy.inspect_string(), "[1, 2]");

        arr.freeze_value().unwrap();
        let cloned = send_value(&arr, Symbol::intern("clone"), &[], None).unwrap();
        assert!(cloned.is_frozen());
    }

    /// First-class Class values -- identity equality, the
    /// universal `.class` reflection through both dispatchers, and the
    /// registry-backed name/ancestors surface.
    #[test]
    fn class_values_reflect_through_the_registry() {
        install();
        let point = RubyValue::Class(Point::CLASS_ID);

        assert!(point.rb_eq(&RubyValue::Class(Point::CLASS_ID)));
        assert!(!point.rb_eq(&RubyValue::Class(Greeter::CLASS_ID)));
        assert_eq!(point.to_display_string(), "Point");
        assert_eq!(point.inspect_string(), "Point");

        let name = send_value(&point, Symbol::intern("name"), &[], None).unwrap();
        assert_eq!(name.to_display_string(), "Point");

        let ancestors = send_value(&point, Symbol::intern("ancestors"), &[], None).unwrap();
        assert_eq!(
            ancestors.inspect_string(),
            "[Point, Object, Kernel, BasicObject]"
        );

        // `.class` on a builtin value, and on an Object through `send`.
        let five_class =
            send_value(&RubyValue::Int(5), Symbol::intern("class"), &[], None).unwrap();
        assert!(five_class.rb_eq(&RubyValue::Class(INTEGER_CLASS)));
        let g: RObj = Greeter::new_handle(std::sync::Arc::new(Greeter {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Greeter::CLASS_ID.0),
        }));
        let g_class = send(&g, Symbol::intern("class"), &[], None).unwrap();
        assert!(g_class.rb_eq(&RubyValue::Class(Greeter::CLASS_ID)));
    }

    /// The registry constructor: `x = Point; x.new(5)` -- allocation plus
    /// the `initialize` dispatch trampoline, driven entirely dynamically.
    #[test]
    fn class_value_new_constructs_through_the_registry() {
        install();
        let point = RubyValue::Class(Point::CLASS_ID);
        let obj = send_value(&point, Symbol::intern("new"), &[RubyValue::Int(9)], None).unwrap();

        let RubyValue::Object(o) = &obj else {
            panic!("constructor must produce an Object")
        };
        assert_eq!(o.class_id(), Point::CLASS_ID);
        let x = send(o, Symbol::intern("x"), &[], None).unwrap();
        assert_eq!(x.to_display_string(), "9");
    }

    /// `Module#===` (`rb_case_eq` with a Class candidate): instance-of
    /// ancestry, NOT equality -- a class never `===`-matches itself.
    #[test]
    fn case_eq_on_a_class_candidate_checks_instance_ancestry() {
        install();
        let point_class = RubyValue::Class(Point::CLASS_ID);
        let instance = RubyValue::Object(Point::new_handle(std::sync::Arc::new(Point {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Point::CLASS_ID.0),
        })));

        assert!(point_class.rb_case_eq(&instance));
        assert!(
            RubyValue::Class(Object::CLASS_ID).rb_case_eq(&instance),
            "ancestry, not identity"
        );
        assert!(
            !point_class.rb_case_eq(&point_class),
            "Widget === Widget is false"
        );
    }

    /// The NoMethodError message cites the registered class NAME (Phase
    /// 16.1, retiring the class-id approximation). No factory installed in
    /// unit tests, so the miss surfaces as the documented loud panic.
    #[test]
    #[should_panic(expected = "undefined method 'nope' for an instance of Point")]
    fn no_method_error_names_the_real_class() {
        install();
        let p: RObj = Point::new_handle(std::sync::Arc::new(Point {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Point::CLASS_ID.0),
        }));
        let _ = send(&p, Symbol::intern("nope"), &[], None);
    }

    /// Comparable, Rust-backed (the compar.c pattern) --
    /// `send`'s fallback drives the includer's own `<=>`.
    #[test]
    fn comparable_methods_drive_the_includers_spaceship() {
        install();
        let a = temp(50).as_object_unchecked();

        let lt = send(&a, Symbol::intern("<"), &[temp(70)], None).unwrap();
        assert!(lt.truthy());
        let gt = send(&a, Symbol::intern(">"), &[temp(70)], None).unwrap();
        assert!(!gt.truthy());
        let between = send(&a, Symbol::intern("between?"), &[temp(40), temp(60)], None).unwrap();
        assert!(between.truthy());

        // clamp returns the BOUND when outside it, the receiver otherwise.
        let clamped = send(&a, Symbol::intern("clamp"), &[temp(55), temp(80)], None).unwrap();
        assert!(clamped.rb_cmp(&temp(55)) == Some(0));
        let kept = send(&a, Symbol::intern("clamp"), &[temp(40), temp(80)], None).unwrap();
        assert!(kept.rb_cmp(&temp(50)) == Some(0));
    }

    /// `rb_eq` resolution order on Objects: Comparable's derived `==`
    /// (via `<=>`) beats identity; identity remains the default without it.
    #[test]
    fn rb_eq_derives_equality_from_comparable() {
        install();
        assert!(temp(50).rb_eq(&temp(50)));
        assert!(!temp(50).rb_eq(&temp(51)));

        // No `==`, no Comparable: reference identity (real `Object#==`).
        let g1 = RubyValue::Object(Greeter::new_handle(std::sync::Arc::new(Greeter {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Greeter::CLASS_ID.0),
        })));
        let g2 = RubyValue::Object(Greeter::new_handle(std::sync::Arc::new(Greeter {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Greeter::CLASS_ID.0),
        })));
        assert!(g1.rb_eq(&g1.clone()));
        assert!(!g1.rb_eq(&g2));
    }

    /// The default Object rendering is CRuby's `#<FQName:0xADDR>` -- the
    /// class name from the registry plus the object's identity address.
    /// `Greeter` declares no ivars, so `inspect` matches `to_s` (no ivar
    /// list); the address is non-deterministic, so this asserts structure.
    #[test]
    fn default_object_rendering_carries_class_and_address() {
        install();
        let g = RubyValue::Object(Greeter::new_handle(std::sync::Arc::new(Greeter {
            __frozen: Default::default(),
            __ivars: IvarCell::new(),
            __class: std::sync::atomic::AtomicU32::new(Greeter::CLASS_ID.0),
        })));
        let s = g.to_display_string();
        assert!(s.starts_with("#<Greeter:0x") && s.ends_with('>'), "got {s}");
        // No ivars -> inspect agrees with to_s.
        assert_eq!(g.inspect_string(), s);
    }

    /// Builtin-reopen value methods. `send_value` consults them
    /// FIRST -- a user `length` override beats the curated String table row
    /// (real Ruby's rule, oracle-verified) -- and `responds_to` sees them.
    #[test]
    fn value_methods_dispatch_first_and_override_curated_rows() {
        install();
        let a = RubyValue::Array(array_new(vec![RubyValue::Int(1)]));

        let shout = send_value(&a, Symbol::intern("shout"), &[], None).unwrap();
        assert_eq!(shout.to_display_string(), "[1]!");

        // The curated `Array#length` row would answer 1; the reopen's
        // override must win.
        let len = send_value(&a, Symbol::intern("length"), &[], None).unwrap();
        assert_eq!(len.to_display_string(), "42");

        assert!(responds_to(ARRAY_CLASS, Symbol::intern("shout"), false));
        assert!(!responds_to(ARRAY_CLASS, Symbol::intern("whisper"), false));
    }

    /// `respond_to?`'s default skips PRIVATE methods (CRuby's rule), and
    /// its `include_all` second argument opts them back in. Kernel's own
    /// C-implemented privates (`puts`/`p`/...) follow the same rule without
    /// a registry entry of their own.
    #[test]
    fn responds_to_skips_private_methods_unless_include_all() {
        install();
        // A public reopen method answers either way.
        let shout = Symbol::intern("shout");
        assert!(responds_to(ARRAY_CLASS, shout, false));
        assert!(responds_to(ARRAY_CLASS, shout, true));

        // Kernel's print family is private: invisible by default, visible
        // with `include_all` -- the same rule `mark_private`-recorded user
        // methods (every top-level `def`) get, which the `top_level_defs`
        // example covers end to end against the ruby oracle.
        let puts = Symbol::intern("puts");
        assert!(!responds_to(ARRAY_CLASS, puts, false));
        assert!(responds_to(ARRAY_CLASS, puts, true));
    }

    /// `display_with`'s value-method probe -- a builtin `to_s`
    /// override drives `puts`/interpolation rendering (and `inspect`'s
    /// catch-all for kinds whose inspect delegates to display).
    #[test]
    fn display_probe_honors_a_builtin_to_s_override() {
        install();
        let q = queue_new();
        assert_eq!(q.to_display_string(), "#<a queue, reopened>");
    }

    /// `Send + Sync` regression guard: fails to compile if
    /// `RubyValue`, `Signal`, or a generated class ever regains an `Rc`/
    /// `RefCell` anywhere in its type graph -- catches the mistake via the
    /// type system immediately, rather than silently reintroducing a
    /// `!Send`/`!Sync` blocker.
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn core_types_are_send_and_sync() {
        assert_send_sync::<RubyValue>();
        assert_send_sync::<Signal>();
        assert_send_sync::<Point>();
        assert_send_sync::<Greeter>();
        assert_send_sync::<RObj>();
        assert_send_sync::<RProc>();
    }
}
