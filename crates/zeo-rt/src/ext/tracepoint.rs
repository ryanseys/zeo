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
//! runs under the `<main>` frame); `callee_id` equals `method_id` for an
//! aliased call. A handler that raises aborts the program with the
//! uncaught-exception report -- which is what CRuby's propagation
//! observably does (probed: even a `rescue` around the traced call does
//! not see the handler's exception).
//!
//! Events fire on every thread; the snapshot a handler reads is
//! thread-local, and events raised while a handler runs are suppressed
//! (CRuby's own reentrancy rule, which is what keeps a `:line` handler
//! from tracing itself forever).

use crate::builtins::{arg_error, arity, class_name_of, runtime_error, type_error};
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
    raised: Option<RubyValue>,
}

thread_local! {
    static IN_HANDLER: Cell<bool> = const { Cell::new(false) };
    static CURRENT: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
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
        label: fr.method,
        raised: None,
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
        raised: None,
    });
}

/// `FrameGuard::drop`'s hook: `:return`/`:end` at the scope's `end` line.
/// Fires on EVERY exit path -- including an exception unwinding through
/// the method, which is CRuby's behavior too (oracle-verified).
#[cold]
pub fn fire_exit(fr: &crate::frames::Frame) {
    dispatch(Snapshot {
        bit: if fr.method.starts_with('<') { END } else { RETURN },
        path: fr.file,
        lineno: fr.end_line,
        label: fr.method,
        raised: None,
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
        label: fr.method,
        raised: Some(exc.clone()),
    });
}

/// Run every enabled tracepoint subscribed to the event. Events raised
/// while a handler runs are suppressed (reentrancy would otherwise make a
/// `:line` handler trace itself forever). A handler that raises aborts the
/// program the way the generated `main` does -- `at_exit`/finalizers, then
/// the uncaught report (or a silent `SystemExit`) -- because a hook called
/// from `set_line`/`Drop` has no `Result` channel to propagate through,
/// and CRuby's propagation is observably fatal too.
fn dispatch(snap: Snapshot) {
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
    if tps.is_empty() {
        return;
    }
    IN_HANDLER.set(true);
    CURRENT.with(|c| *c.borrow_mut() = Some(snap));
    for tp in tps {
        let rtp = tp_of(&tp);
        // An earlier handler this event may have disabled a later one.
        if !rtp.enabled.load(Ordering::Relaxed) {
            continue;
        }
        match crate::catch_break(rtp.block.call(std::slice::from_ref(&tp))) {
            Ok(_) => {}
            Err(Signal::Raise(exc)) => {
                crate::run_at_exit();
                crate::run_finalizers();
                if let Some(code) = crate::system_exit_status(&exc) {
                    std::process::exit(code);
                }
                crate::report_uncaught(&exc);
                std::process::exit(1);
            }
            Err(_) => {}
        }
    }
    CURRENT.with(|c| *c.borrow_mut() = None);
    IN_HANDLER.set(false);
}

fn register(tp: &RubyValue) {
    tp_of(tp).enabled.store(true, Ordering::Relaxed);
    let mut active = ACTIVE.lock().unwrap();
    if !active.iter().any(|t| same_tp(t, tp)) {
        active.push(tp.clone());
    }
    TRACING.store(true, Ordering::Relaxed);
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
    if label.starts_with("block (") {
        if let Some(i) = label.find(") in ") {
            return &label[i + 5..];
        }
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
                    class_name_of(other)
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

    def self."new" (_recv, args, block) {
        new_tp(args, &block)
    }
    // `trace` is `new` + `enable` in one step.
    def self."trace" (_recv, args, block) {
        let tp = new_tp(args, &block)?;
        register(&tp);
        Ok(tp)
    }

    // `enable`/`disable` answer the PREVIOUS state (oracle-verified: a
    // fresh `enable` is false). The block forms restore that state on the
    // way out -- also past a raise -- and answer the block's value.
    def "enable" (recv, args, block) {
        arity!(args, 0);
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
    def "disable" (recv, args, block) {
        arity!(args, 0);
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
    def "enabled?" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(tp_of(recv).enabled.load(Ordering::Relaxed)))
    }

    def "event" (recv, args, _block) {
        arity!(args, 0);
        let _ = tp_of(recv);
        Ok(RubyValue::Symbol(Symbol::intern(event_name(snapshot()?.bit))))
    }
    def "lineno" (recv, args, _block) {
        arity!(args, 0);
        let _ = tp_of(recv);
        Ok(RubyValue::Int(i64::from(snapshot()?.lineno)))
    }
    def "path" (recv, args, _block) {
        arity!(args, 0);
        let _ = tp_of(recv);
        Ok(RubyValue::Str(string_new(snapshot()?.path.to_string())))
    }
    // `callee_id` is `method_id` here: zeo's frame labels carry the
    // defining name, and an aliased call is not distinguished.
    def "method_id" | "callee_id" (recv, args, _block) {
        arity!(args, 0);
        let _ = tp_of(recv);
        Ok(method_symbol(snapshot()?.label))
    }
    def "defined_class" (recv, args, _block) {
        arity!(args, 0);
        let _ = tp_of(recv);
        Ok(resolve_defined_class(snapshot()?.label))
    }
    def "raised_exception" (recv, args, _block) {
        arity!(args, 0);
        let _ = tp_of(recv);
        let snap = snapshot()?;
        if snap.bit != RAISE {
            return Err(runtime_error!("not supported by this event"));
        }
        Ok(snap.raised.unwrap_or(RubyValue::Nil))
    }

    // Outside a handler: `#<TracePoint:enabled>`/`#<TracePoint:disabled>`
    // (no address -- CRuby's own shape). Inside one, the current event:
    // `#<TracePoint:call 'volume' f.rb:9>`.
    def "inspect" | "to_s" (recv, args, _block) {
        arity!(args, 0);
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
        assert_eq!(label_parts("Speaker#volume"), Some(("Speaker", false, "volume")));
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
                raised: None,
            });
        });
        assert_eq!(snapshot().unwrap().lineno, 3);
        CURRENT.with(|c| *c.borrow_mut() = None);
    }
}
