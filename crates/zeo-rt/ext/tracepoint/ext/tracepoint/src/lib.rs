//! `TracePoint` -- execution tracing over the instrumentation the runtime
//! already carries for backtraces: `set_line` fires `:line`,
//! `FrameGuard::push`/`Drop` fire `:call`/`:return` (and `:class`/`:end`
//! for class-body frames, classified by the frame's label), and
//! `attach_backtrace` fires `:raise`. There is no separate tracing VM mode:
//! when no tracepoint is enabled the hooks cost one relaxed atomic load.
//!
//! The reachable subset (oracle-verified against CRuby):
//! `:line`/`:call`/`:return`/`:class`/`:end`/`:raise`, with `event`,
//! `path`, `lineno`, `method_id`, `callee_id`, `defined_class`, and
//! `raised_exception`. CRuby-valid events zeo cannot fire (`:b_call`,
//! `:c_call`, ...) raise loudly at `new` rather than silently never
//! firing. `#self`, `#binding`, and `#return_value` need a receiver on the
//! frame, which the lightweight `Frame` deliberately does not carry.
//!
//! Divergences (see COMPATIBILITY.md): an explicit early `return` reports
//! the method's `end` line (a `Drop` cannot tell the exit paths apart);
//! `def` lines fire no `:line` (definitions are compile-time); top-level
//! lines of a required file report the entry file's path (spliced code
//! runs under the `<main>` frame). A handler that raises aborts the program with the
//! uncaught-exception report -- which is what CRuby's propagation
//! observably does (probed: even a `rescue` around the traced call does
//! not see the handler's exception).
//!
//! Events fire on every thread; the snapshot a handler reads is
//! thread-local, and events raised while a handler runs are suppressed
//! (CRuby's own reentrancy rule, which is what keeps a `:line` handler
//! from tracing itself forever).

use crate::builtins::{arg_error, runtime_error, type_error};
use crate::collections::string_new;
use crate::dispatch::{RObj, RubyObject};
use crate::{RProc, RubyValue, Signal, Symbol};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use zeo_abi::TRACEPOINT_CLASS;
use zeo_macros::ruby_class;

const LINE: u8 = 1;
const CALL: u8 = 2;
const RETURN: u8 = 4;
const CLASS: u8 = 8;
const END: u8 = 16;
const RAISE: u8 = 32;
const ALL: u8 = LINE | CALL | RETURN | CLASS | END | RAISE;

/// CRuby accepts these; zeo has no hook that can fire them, and accepting
/// one would mean a handler that silently never runs -- so `new` raises
/// loudly instead (the `Coverage` unsupported-mode precedent).
const UNSUPPORTED: &[&str] = &[
    "b_call",
    "b_return",
    "c_call",
    "c_return",
    "rescue",
    "thread_begin",
    "thread_end",
    "fiber_switch",
    "script_compiled",
];

fn event_bit(name: &str) -> Result<u8, Signal> {
    match name {
        "line" => Ok(LINE),
        "call" => Ok(CALL),
        "return" => Ok(RETURN),
        "class" => Ok(CLASS),
        "end" => Ok(END),
        "raise" => Ok(RAISE),
        _ if UNSUPPORTED.contains(&name) => {
            Err(runtime_error!("event :{name} is not supported by zeo"))
        }
        _ => Err(arg_error!("unknown event: {name}")),
    }
}

fn event_name(bit: u8) -> &'static str {
    match bit {
        LINE => "line",
        CALL => "call",
        RETURN => "return",
        CLASS => "class",
        END => "end",
        _ => "raise",
    }
}

pub struct RTracePoint {
    events: u8,
    block: RProc,
    enabled: AtomicBool,
    frozen: AtomicBool,
}

impl RubyObject for RTracePoint {
    fn class_id(&self) -> crate::ClassId {
        TRACEPOINT_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        // A dup is a fresh, DISABLED tracepoint over the same events and
        // handler -- enablement is registry membership, which never copies.
        let tp = RTracePoint {
            events: self.events,
            block: self.block.clone(),
            enabled: AtomicBool::new(false),
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            tp.set_frozen();
        }
        Arc::new(tp)
    }
}

/// The `RTracePoint` behind a receiver -- the table only dispatches on one.
fn tp_of(recv: &RubyValue) -> &RTracePoint {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RTracePoint>()
            .expect("the TracePoint table only dispatches on TracePoint receivers"),
        _ => unreachable!("the TracePoint table only dispatches on TracePoint receivers"),
    }
}

/// The fast gate the frame/line/raise hooks load before doing anything
/// else: true exactly while at least one tracepoint is enabled.
static TRACING: AtomicBool = AtomicBool::new(false);

/// The enabled tracepoints, in enable order; dispatch runs them NEWEST
/// FIRST (oracle-verified -- CRuby prepends each hook). The lock is only
/// ever held to snapshot or edit the list -- never across a handler call,
/// whose Ruby code may enable/disable tracepoints itself.
static ACTIVE: LazyLock<Mutex<Vec<RubyValue>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// What a handler's accessors read, valid exactly while a handler runs on
/// this thread ("access from outside" otherwise, CRuby's error).
#[derive(Clone)]
struct Snapshot {
    bit: u8,
    path: &'static str,
    lineno: u32,
    /// The frame label the event fired under (`Class#m`, `<class:X>`,
    /// `block in Class#m`, `<main>`); `method_id`/`defined_class` parse it
    /// lazily so the fire path never allocates.
    label: &'static str,
    /// The name a run-time alias called the traced method through, read off
    /// its frame when the event fires ([`crate::frames::current_frame_callee`]);
    /// `None` when the call used the method's own name.
    callee: Option<crate::Symbol>,
    raised: Option<RubyValue>,
    /// The receiver the event ran under (`TracePoint#self`, and the degraded
    /// `#binding`'s receiver) -- resolved by `dispatch` from the armed-only
    /// self notes, `main` when no note covers the frame.
    slf: RubyValue,
}

thread_local! {
    static IN_HANDLER: Cell<bool> = const { Cell::new(false) };
    static CURRENT: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    /// `(frame depth, receiver)` notes, pushed by compiled method prologues
    /// ONLY while tracing is armed (`trace_frame_self`'s gate) -- the frame
    /// itself stays 40 bytes and the untraced path pays one relaxed load.
    /// Depth-keyed with prune-on-push, so a note left behind by a returned
    /// method is displaced the next time anything notes at or below it.
    static SELF_STACK: RefCell<Vec<(usize, RubyValue)>> = const { RefCell::new(Vec::new()) };
}

/// The compiled-prologue self note: evaluate `f` (boxing the receiver) only
/// while a tracepoint or legacy trace hook is armed.
#[inline]
pub fn trace_frame_self(f: impl FnOnce() -> RubyValue) {
    if tracing() {
        note_self(f());
    }
}

#[cold]
fn note_self(v: RubyValue) {
    let depth = crate::frames::depth();
    SELF_STACK.with(|s| {
        let mut s = s.borrow_mut();
        while s.last().is_some_and(|&(d, _)| d >= depth) {
            s.pop();
        }
        s.push((depth, v));
    });
}

/// The innermost note at or below the current frame depth -- a block's
/// events resolve to its enclosing method's receiver (a block shares its
/// method's `self`, `instance_exec` rebinding aside). `main` when nothing
/// noted (the top level, or a frame entered before arming).
fn current_self() -> RubyValue {
    let depth = crate::frames::depth();
    SELF_STACK
        .with(|s| {
            s.borrow()
                .iter()
                .rev()
                .find(|&&(d, _)| d <= depth)
                .map(|(_, v)| v.clone())
        })
        .unwrap_or_else(crate::dispatch::main_object)
}

#[inline]
pub fn tracing() -> bool {
    TRACING.load(Ordering::Relaxed)
}

/// `set_line`'s hook: a statement began on `line`. Path and label come
/// from the innermost frame (read here, on the traced slow path, so the
/// stamp itself stays two TLS accesses). The `fire_*` family is
/// `#[cold]`: the hooks inline into every generated method, and only the
/// one-load gate belongs in that fast path.
#[cold]
pub fn fire_line(line: u32) {
    let Some(fr) = crate::frames::current_frame() else {
        return;
    };
    dispatch(Snapshot {
        bit: LINE,
        path: fr.file,
        lineno: line,
        label: fr.method(),
        callee: crate::frames::current_frame_callee(),
        raised: None,
        slf: RubyValue::Nil,
    });
}

/// `FrameGuard::push`'s hook: method entry (`:call`, at the `def` line) or
/// class-body entry (`:class`, at the `class`/`module` line).
#[cold]
pub fn fire_entry(file: &'static str, label: &'static str, line: u32) {
    dispatch(Snapshot {
        bit: if label.starts_with('<') { CLASS } else { CALL },
        path: file,
        lineno: line,
        label,
        // The frame this event announces is the one just pushed.
        callee: crate::frames::current_frame_callee(),
        raised: None,
        slf: RubyValue::Nil,
    });
}

/// `FrameGuard::drop`'s hook: `:return`/`:end` at the scope's `end` line.
/// Fires on EVERY exit path -- including an exception unwinding through
/// the method, which is CRuby's behavior too (oracle-verified).
#[cold]
pub fn fire_exit(fr: &crate::frames::Frame) {
    dispatch(Snapshot {
        bit: if fr.method().starts_with('<') {
            END
        } else {
            RETURN
        },
        path: fr.file,
        lineno: fr.end_line,
        label: fr.method(),
        callee: (fr.callee != 0).then(|| crate::Symbol::from_u32(fr.callee - 1)),
        raised: None,
        slf: RubyValue::Nil,
    });
}

/// CRuby implements TracePoint's whole surface in Ruby -- `trace_point.rb`,
/// compiled into the VM, each body a `Primitive.` call -- so calling one of
/// them fires an ordinary `:call`/`:return` pair, at that file's path and its
/// own `def`/`end` lines. A program tracing `:call` sees them: `tp.disable`
/// reports one last `:disable` before the trace stops. zeo's rows are native
/// and would fire nothing, so each injects the pair by hand.
///
/// The gate is read at each end INDEPENDENTLY, which is what puts a pair's
/// two halves where CRuby puts them: the `disable` that ends a trace fires
/// `:call` and no `:return`, and the `enable` that starts one fires `:return`
/// and no `:call`. Only the rows ordinary code calls are instrumented -- the event
/// accessors are legal only inside a handler, where reentrancy suppresses
/// everything anyway.
///
/// Lines are ruby 4.0.6's; re-read them from `trace_point.rb` when the oracle
/// moves.
const INTERNAL_PATH: &str = "<internal:trace_point>";

struct InternalFrame {
    label: &'static str,
    end_line: u32,
    slf: RubyValue,
}

impl InternalFrame {
    fn enter(label: &'static str, lines: (u32, u32), slf: &RubyValue) -> Self {
        if tracing() {
            fire_internal(CALL, label, lines.0, slf);
        }
        Self {
            label,
            end_line: lines.1,
            slf: slf.clone(),
        }
    }
}

impl Drop for InternalFrame {
    fn drop(&mut self) {
        if tracing() {
            fire_internal(RETURN, self.label, self.end_line, &self.slf);
        }
    }
}

#[cold]
fn fire_internal(bit: u8, label: &'static str, lineno: u32, slf: &RubyValue) {
    dispatch(Snapshot {
        bit,
        path: INTERNAL_PATH,
        lineno,
        label,
        callee: None,
        raised: None,
        slf: slf.clone(),
    });
}

/// `attach_backtrace`'s hook: `:raise` at the raising frame's current
/// line, carrying the exception for `#raised_exception`.
#[cold]
pub fn fire_raise(exc: &RubyValue) {
    let Some(fr) = crate::frames::current_frame() else {
        return;
    };
    dispatch(Snapshot {
        bit: RAISE,
        path: fr.file,
        lineno: fr.line,
        label: fr.method(),
        callee: crate::frames::current_frame_callee(),
        raised: Some(exc.clone()),
        slf: RubyValue::Nil,
    });
}

/// Run every enabled tracepoint subscribed to the event. Events raised
/// while a handler runs are suppressed (reentrancy would otherwise make a
/// `:line` handler trace itself forever). A handler that raises aborts the
/// program the way the generated `main` does -- `at_exit`/finalizers, then
/// the uncaught report (or a silent `SystemExit`) -- because a hook called
/// from `set_line`/`Drop` has no `Result` channel to propagate through,
/// and CRuby's propagation is observably fatal too.
fn dispatch(mut snap: Snapshot) {
    if IN_HANDLER.get() {
        return;
    }
    let bit = snap.bit;
    let tps: Vec<RubyValue> = ACTIVE
        .lock()
        .unwrap()
        .iter()
        .rev()
        .filter(|tp| tp_of(tp).events & bit != 0)
        .cloned()
        .collect();
    let legacy = LEGACY.lock().unwrap().clone();
    if tps.is_empty() && legacy.is_none() {
        return;
    }
    // An injected `<internal:trace_point>` frame supplies its own receiver
    // (the tracepoint itself); every other hook leaves it Nil for the notes.
    if matches!(snap.slf, RubyValue::Nil) {
        snap.slf = current_self();
    }
    IN_HANDLER.set(true);
    CURRENT.with(|c| *c.borrow_mut() = Some(snap.clone()));
    let fatal = |exc: RubyValue| -> ! {
        crate::run_at_exit();
        crate::run_finalizers();
        if let Some(code) = crate::system_exit_status(&exc) {
            std::process::exit(code);
        }
        crate::report_uncaught(&exc);
        std::process::exit(1);
    };
    for tp in tps {
        let rtp = tp_of(&tp);
        // An earlier handler this event may have disabled a later one.
        if !rtp.enabled.load(Ordering::Relaxed) {
            continue;
        }
        match crate::catch_break(rtp.block.call(std::slice::from_ref(&tp))) {
            Ok(_) => {}
            Err(Signal::Raise(exc)) => fatal(exc),
            Err(_) => {}
        }
    }
    // The `set_trace_func` legacy hook, over the same events, with CRuby's
    // six arguments `(event, file, line, id, binding, classname)`. The
    // binding is the degraded receiver-only capture `TracePoint#binding`
    // hands out; `c-call`/`c-return` never fire (no hook exists to fire
    // them from -- documented).
    if let Some(hook) = legacy {
        let args = [
            RubyValue::Str(string_new(event_name(bit).to_string())),
            RubyValue::Str(string_new(snap.path.to_string())),
            RubyValue::Int(i64::from(snap.lineno)),
            method_symbol(snap.label),
            degraded_binding(&snap),
            resolve_defined_class(snap.label),
        ];
        match crate::catch_break(hook.call(&args)) {
            Ok(_) => {}
            Err(Signal::Raise(exc)) => fatal(exc),
            Err(_) => {}
        }
    }
    CURRENT.with(|c| *c.borrow_mut() = None);
    IN_HANDLER.set(false);
}

/// The `set_trace_func` hook slot -- one global Proc (CRuby replaces on each
/// call; `nil` clears). Firing rides [`dispatch`], so arming it arms the
/// same [`TRACING`] gate the tracepoints use.
static LEGACY: LazyLock<Mutex<Option<RProc>>> = LazyLock::new(|| Mutex::new(None));

pub fn set_legacy_hook(hook: Option<RProc>) {
    let armed = hook.is_some();
    *LEGACY.lock().unwrap() = hook;
    if armed {
        TRACING.store(true, Ordering::Relaxed);
        // Emitted prologues must start CALLING their frame pushes so the
        // events actually fire (the inline stores skip them).
        crate::runtime_meta::arm_frames_indirect();
    } else {
        TRACING.store(!ACTIVE.lock().unwrap().is_empty(), Ordering::Relaxed);
    }
}

/// The receiver-only `Binding` a trace event can supply: `#receiver` and
/// `#eval` against the event's `self`, NO locals (a real frame binding
/// would force cell storage on every local in every traced program --
/// documented in COMPATIBILITY.md).
fn degraded_binding(snap: &Snapshot) -> RubyValue {
    crate::binding_new(
        snap.slf.clone(),
        Vec::new(),
        snap.path,
        snap.lineno,
        0,
        u32::MAX,
    )
}

fn register(tp: &RubyValue) {
    tp_of(tp).enabled.store(true, Ordering::Relaxed);
    let mut active = ACTIVE.lock().unwrap();
    if !active.iter().any(|t| same_tp(t, tp)) {
        active.push(tp.clone());
    }
    TRACING.store(true, Ordering::Relaxed);
    // See `set_legacy_hook`: inline frame prologues skip the events.
    crate::runtime_meta::arm_frames_indirect();
}

fn unregister(tp: &RubyValue) {
    tp_of(tp).enabled.store(false, Ordering::Relaxed);
    let mut active = ACTIVE.lock().unwrap();
    active.retain(|t| !same_tp(t, tp));
    TRACING.store(!active.is_empty(), Ordering::Relaxed);
}

fn same_tp(a: &RubyValue, b: &RubyValue) -> bool {
    match (a, b) {
        (RubyValue::Object(x), RubyValue::Object(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

fn snapshot() -> Result<Snapshot, Signal> {
    CURRENT
        .with(|c| c.borrow().clone())
        .ok_or_else(|| runtime_error!("access from outside"))
}

/// `block (2 levels) in Class#m` -> `Class#m`: a block's `method_id` and
/// `defined_class` are its enclosing method's (oracle-verified).
fn strip_block_prefix(label: &str) -> &str {
    if let Some(rest) = label.strip_prefix("block in ") {
        return rest;
    }
    if label.starts_with("block (")
        && let Some(i) = label.find(") in ")
    {
        return &label[i + 5..];
    }
    label
}

/// `(class_path, is_singleton, method_name)` from a frame label, `None`
/// for the label shapes that carry no method (`<main>`, `<class:X>`).
fn label_parts(label: &str) -> Option<(&str, bool, &str)> {
    let l = strip_block_prefix(label);
    if l.starts_with('<') {
        return None;
    }
    let i = l.rfind(['#', '.'])?;
    Some((&l[..i], l.as_bytes()[i] == b'.', &l[i + 1..]))
}

/// The `Class` behind a label's fully-qualified class path (the registry
/// stores classes under exactly these names); the singleton class of it
/// for a `Class.m` label (CRuby's `#<Class:Foo>`). Nil when the name does
/// not resolve.
fn resolve_defined_class(label: &str) -> RubyValue {
    let Some((path, singleton, _)) = label_parts(label) else {
        return RubyValue::Nil;
    };
    let Some(cid) = crate::dispatch::class_id_by_name(path) else {
        return RubyValue::Nil;
    };
    let class = RubyValue::Class(cid);
    if singleton {
        crate::runtime_meta::runtime_singleton_class(&class).unwrap_or(RubyValue::Nil)
    } else {
        class
    }
}

fn method_symbol(label: &str) -> RubyValue {
    match label_parts(label) {
        Some((_, _, name)) => RubyValue::Symbol(Symbol::intern(name)),
        None => RubyValue::Nil,
    }
}

fn parse_events(args: &[RubyValue]) -> Result<u8, Signal> {
    let mut mask: u8 = 0;
    for a in args {
        let name = match a {
            RubyValue::Symbol(s) => s.name().to_string(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => {
                return Err(type_error!(
                    "wrong argument type {} (expected Symbol)",
                    crate::builtins::check_type_name(other)
                ));
            }
        };
        mask |= event_bit(&name)?;
    }
    Ok(if mask == 0 { ALL } else { mask })
}

fn new_tp(args: &[RubyValue], block: &Option<RubyValue>) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(blk)) = block else {
        return Err(arg_error!("must be called with a block"));
    };
    Ok(RubyValue::Object(Arc::new(RTracePoint {
        events: parse_events(args)?,
        block: blk.clone(),
        enabled: AtomicBool::new(false),
        frozen: AtomicBool::new(false),
    })))
}

ruby_class! {
    TracePoint = zeo_abi::TRACEPOINT_CLASS < zeo_abi::OBJECT_CLASS;

    def self."new" params "*events" (recv, *args, &block) {
        let _fr = InternalFrame::enter("TracePoint.new", (96, 99), recv);
        new_tp(args, &block)
    }
    // `trace` is `new` + `enable` in one step.
    def self."trace" params "*events" (recv, *args, &block) {
        let _fr = InternalFrame::enter("TracePoint.trace", (134, 137), recv);
        let tp = new_tp(args, &block)?;
        register(&tp);
        Ok(tp)
    }

    // `enable`/`disable` answer the PREVIOUS state (oracle-verified: a
    // fresh `enable` is false). The block forms restore that state on the
    // way out -- also past a raise -- and answer the block's value.
    def "enable" params "target: nil, target_line: nil, target_thread: nil" cfunc (recv, &block) {
        let _fr = InternalFrame::enter("TracePoint#enable", (261, 264), recv);
        let prev = tp_of(recv).enabled.load(Ordering::Relaxed);
        match &block {
            Some(RubyValue::Proc(p)) => {
                let p = p.clone();
                if !prev {
                    register(recv);
                }
                let r = crate::catch_break(p.call(&[]));
                if !prev {
                    unregister(recv);
                }
                r
            }
            _ => {
                if !prev {
                    register(recv);
                }
                Ok(RubyValue::Bool(prev))
            }
        }
    }
    def "disable" (recv, &block) {
        let _fr = InternalFrame::enter("TracePoint#disable", (297, 300), recv);
        let prev = tp_of(recv).enabled.load(Ordering::Relaxed);
        match &block {
            Some(RubyValue::Proc(p)) => {
                let p = p.clone();
                if prev {
                    unregister(recv);
                }
                let r = crate::catch_break(p.call(&[]));
                if prev {
                    register(recv);
                }
                r
            }
            _ => {
                if prev {
                    unregister(recv);
                }
                Ok(RubyValue::Bool(prev))
            }
        }
    }
    def "enabled?" (recv) {
        let _fr = InternalFrame::enter("TracePoint#enabled?", (306, 308), recv);
        Ok(RubyValue::Bool(tp_of(recv).enabled.load(Ordering::Relaxed)))
    }

    def "event" (recv) {
        let _ = tp_of(recv);
        Ok(RubyValue::Symbol(Symbol::intern(event_name(snapshot()?.bit))))
    }
    def "lineno" (recv) {
        let _ = tp_of(recv);
        Ok(RubyValue::Int(i64::from(snapshot()?.lineno)))
    }
    def "path" (recv) {
        let _ = tp_of(recv);
        Ok(RubyValue::Str(string_new(snapshot()?.path.to_string())))
    }
    def "method_id" (recv) {
        let _ = tp_of(recv);
        Ok(method_symbol(snapshot()?.label))
    }
    // The name the call used: a run-time alias's, when one handed it to the
    // traced method's frame, else the defining name the label carries.
    def "callee_id" (recv) {
        let _ = tp_of(recv);
        let snap = snapshot()?;
        Ok(match snap.callee {
            Some(sym) => RubyValue::Symbol(sym),
            None => method_symbol(snap.label),
        })
    }
    def "defined_class" (recv) {
        let _ = tp_of(recv);
        Ok(resolve_defined_class(snapshot()?.label))
    }
    def "raised_exception" (recv) {
        let _ = tp_of(recv);
        let snap = snapshot()?;
        if snap.bit != RAISE {
            return Err(runtime_error!("not supported by this event"));
        }
        Ok(snap.raised.unwrap_or(RubyValue::Nil))
    }

    // `#self`/`#binding` read the armed-only self notes: the receiver the
    // event ran under, and a receiver-only Binding over it (see
    // `degraded_binding`).
    def "self" (recv) {
        let _ = tp_of(recv);
        Ok(snapshot()?.slf)
    }
    def "binding" (recv) {
        let _ = tp_of(recv);
        Ok(degraded_binding(&snapshot()?))
    }
    // The remaining frame-dependent accessors. zeo's `Frame` is deliberately
    // lightweight -- a file, a line and a label -- so it carries no return
    // value and no compiled sequence. Each raises CRuby's OWN refusal for an
    // event that cannot supply them, which is what a `:line` event gets from
    // CRuby for these four. Refusing loudly is the rule `TracePoint.new`
    // already follows for the events zeo cannot fire.
    def "return_value" | "parameters"
        | "eval_script" | "instruction_sequence" (recv) {
        let _ = tp_of(recv);
        // "access from outside" outranks it, exactly as in CRuby: a handler
        // has to be running before the event can be the wrong one.
        let _ = snapshot()?;
        Err(runtime_error!("not supported by this event"))
    }

    // `TracePoint.stat` reports per-VM hook counts, keyed by a `RubyVM`
    // object zeo has none of, so there is nothing to key on and nothing to
    // count.
    def self."stat" (recv) {
        let _fr = InternalFrame::enter("TracePoint.stat", (119, 121), recv);
        Ok(RubyValue::Hash(crate::collections::hash_new(Vec::new())))
    }
    // `TracePoint.allow_reentry { }` re-arms tracing inside a handler.
    // Outside one CRuby refuses; inside one, zeo's reentrancy suppression
    // stays on (a handler that traced itself would not terminate), so the
    // block simply runs.
    def self."allow_reentry" (recv, &block) {
        let _fr = InternalFrame::enter("TracePoint.allow_reentry", (200, 203), recv);
        let Some(block) = block else {
            return Err(runtime_error!("must be called with a block"));
        };
        if CURRENT.with(|c| c.borrow().is_none()) {
            return Err(runtime_error!("No need to allow reentrance."));
        }
        block.as_proc_unchecked().call(&[])
    }

    // Outside a handler: `#<TracePoint:enabled>`/`#<TracePoint:disabled>`
    // (no address -- CRuby's own shape). Inside one, the current event:
    // `#<TracePoint:call 'volume' f.rb:9>`.
    def "inspect" (recv) {
        let _fr = InternalFrame::enter("TracePoint#inspect", (106, 108), recv);
        let s = match CURRENT.with(|c| c.borrow().clone()) {
            Some(snap) => match label_parts(snap.label) {
                Some((_, _, name)) => format!(
                    "#<TracePoint:{} '{}' {}:{}>",
                    event_name(snap.bit),
                    name,
                    snap.path,
                    snap.lineno
                ),
                None => format!(
                    "#<TracePoint:{} {}:{}>",
                    event_name(snap.bit),
                    snap.path,
                    snap.lineno
                ),
            },
            None if tp_of(recv).enabled.load(Ordering::Relaxed) => {
                "#<TracePoint:enabled>".to_string()
            }
            None => "#<TracePoint:disabled>".to_string(),
        };
        Ok(RubyValue::Str(string_new(s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_the_tracepoint_surface() {
        assert!(lookup("enable").is_some());
        assert!(lookup("raised_exception").is_some());
        assert!(lookup("nope").is_none());
        assert!(lookup_class("new").is_some());
        assert!(lookup_class("trace").is_some());
    }

    #[test]
    fn event_bits_cover_the_reachable_set() {
        assert_eq!(event_bit("line").unwrap(), LINE);
        assert_eq!(event_bit("raise").unwrap(), RAISE);
        assert_eq!(
            LINE | CALL | RETURN | CLASS | END | RAISE,
            ALL,
            "ALL is exactly the reachable set"
        );
        assert_eq!(event_name(CALL), "call");
    }

    #[test]
    fn labels_parse_to_method_and_class_parts() {
        assert_eq!(
            label_parts("Speaker#volume"),
            Some(("Speaker", false, "volume"))
        );
        assert_eq!(label_parts("A::B.build"), Some(("A::B", true, "build")));
        assert_eq!(
            label_parts("block in Widget#scale"),
            Some(("Widget", false, "scale"))
        );
        assert_eq!(
            label_parts("block (2 levels) in Widget#scale"),
            Some(("Widget", false, "scale"))
        );
        assert_eq!(label_parts("<main>"), None);
        assert_eq!(label_parts("<class:Later>"), None);
        assert_eq!(label_parts("block in <main>"), None);
    }

    #[test]
    fn snapshot_access_is_scoped_to_a_handler() {
        // Thread-local, so parallel tests cannot interfere.
        assert!(CURRENT.with(|c| c.borrow().is_none()));
        CURRENT.with(|c| {
            *c.borrow_mut() = Some(Snapshot {
                bit: LINE,
                path: "f.rb",
                lineno: 3,
                label: "<main>",
                callee: None,
                raised: None,
                slf: RubyValue::Nil,
            });
        });
        assert_eq!(snapshot().unwrap().lineno, 3);
        CURRENT.with(|c| *c.borrow_mut() = None);
    }
}
