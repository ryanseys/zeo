//! `RubyVM`, `RubyVM::YJIT`, and `RubyVM::InstructionSequence` -- CRuby's VM
//! introspection namespace mapped onto zeo's own machinery.
//!
//! `RubyVM.stat` answers zeo's REAL counters where a truthful equivalent
//! exists (the constant epoch, constant-cache misses, the cvar write
//! counter) and the truthful cardinality 0 where the concept has no zeo
//! analog (shapes). `YJIT` is present and permanently disabled -- zeo is an
//! AOT compiler, so `enable` truthfully answers false (CRuby answers true;
//! the one documented divergence in this family). `InstructionSequence`
//! really compiles (a prism parse check) and really evaluates;
//! the YARV serialization surface (`to_a`/`to_binary`/`disasm`) refuses with
//! `NotImplementedError` naming the AOT reality -- there is no bytecode.
//!
//! `RubyVM::AbstractSyntaxTree` and its `Node`/`Location` live in
//! `rubyvm_ast.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::Mutex as PlMutex;

use crate::builtins::not_impl_error;
use crate::dispatch::{RObj, RubyObject};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, RUBYVM_ISEQ_CLASS};
use zeo_macros::{ruby_class, ruby_module};

/// `RubyVM.keep_script_lines` -- consulted as the parse-time default by
/// `AbstractSyntaxTree.parse` when no kwarg overrides it.
pub(crate) static KEEP_SCRIPT_LINES: AtomicBool = AtomicBool::new(false);

/// Constant-cache misses -- bumped by the constant slow path.
pub(crate) static CONST_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

/// The global cvar generation -- bumped on every class-variable write
/// (CRuby seeds it at 1).
pub(crate) static GLOBAL_CVAR_STATE: AtomicU64 = AtomicU64::new(1);

fn stat_pairs() -> Vec<(RubyValue, RubyValue)> {
    let sym = |s: &str| RubyValue::Symbol(crate::Symbol::intern(s));
    vec![
        (
            sym("constant_cache_invalidations"),
            RubyValue::Int(crate::constants::const_epoch() as i64),
        ),
        (
            sym("constant_cache_misses"),
            RubyValue::Int(CONST_CACHE_MISSES.load(Ordering::Relaxed) as i64),
        ),
        (
            sym("global_cvar_state"),
            RubyValue::Int(GLOBAL_CVAR_STATE.load(Ordering::Relaxed) as i64),
        ),
        // zeo has no shape system; zero is the truthful cardinality.
        (sym("next_shape_id"), RubyValue::Int(0)),
        (sym("shape_cache_size"), RubyValue::Int(0)),
    ]
}

mod vm {
    use super::*;

    ruby_class! {
        RubyVM = zeo_abi::RUBYVM_CLASS < zeo_abi::OBJECT_CLASS;

        // zeo's real pipeline stages -- the honest analog of CRuby's compiler
        // option strings.
        const OPTS = {
            RubyValue::Array(crate::array_new(vec![
                RubyValue::Str(crate::string_new("ahead-of-time compilation".to_string())),
                RubyValue::Str(crate::string_new("prism parser".to_string())),
                RubyValue::Str(crate::string_new("rustc code generation".to_string())),
            ]))
        };
        // zeo executes native code, not a bytecode instruction set; the empty
        // frozen Array is the truthful answer.
        const INSTRUCTION_NAMES = {
            let a = crate::array_new(vec![]);
            a.set_frozen();
            RubyValue::Array(a)
        };
        const DEFAULT_PARAMS = {
            let int = RubyValue::Int;
            let sym = |s: &str| RubyValue::Symbol(crate::Symbol::intern(s));
            let h = crate::hash_new(vec![
                // Real numbers where zeo has them: threads and ractors spawn
                // with 8MiB stacks (thread.rs); fibers use corosensei's 128KiB.
                (sym("thread_vm_stack_size"), int(1048576)),
                (sym("thread_machine_stack_size"), int(8 * 1024 * 1024)),
                (sym("fiber_vm_stack_size"), int(131072)),
                (sym("fiber_machine_stack_size"), int(131072)),
            ]);
            h.set_frozen();
            RubyValue::Hash(h)
        };

        def self."stat"(_recv, key?) {
            let pairs = stat_pairs();
            match key {
                None => Ok(RubyValue::Hash(crate::hash_new(pairs))),
                Some(RubyValue::Symbol(want)) => {
                    for (k, v) in pairs {
                        if matches!(k, RubyValue::Symbol(s) if s == *want) {
                            return Ok(v);
                        }
                    }
                    Err(crate::builtins::arg_error!("unknown key: {}", want.name()))
                }
                Some(RubyValue::Hash(h)) => {
                    // The fill-this-Hash form (GC.stat's shape).
                    for (k, v) in pairs {
                        crate::collections::hash_set(h, k, v);
                    }
                    Ok(RubyValue::Hash(h.clone()))
                }
                Some(other) => Err(crate::builtins::type_error!(
                    "non-symbol key given: {}",
                    crate::builtins::class_name_of(other)
                )),
            }
        }
        def self."keep_script_lines"(_recv) {
            Ok(RubyValue::Bool(KEEP_SCRIPT_LINES.load(Ordering::Relaxed)))
        }
        def self."keep_script_lines="(_recv, value) {
            KEEP_SCRIPT_LINES.store(value.truthy(), Ordering::Relaxed);
            Ok(value.clone())
        }
    }
}

mod yjit {
    use super::*;

    ruby_module! {
        YJIT = zeo_abi::RUBYVM_YJIT_MODULE;

        // zeo is an AOT compiler: there is no JIT to switch on, so `enable`
        // truthfully answers false where CRuby answers true (the one divergence
        // in this family); every stats/log reader answers its disabled shape.
        def self."enabled?" | "stats_enabled?" | "log_enabled?" | "trace_exit_locations_enabled?" (_recv) {
            Ok(RubyValue::Bool(false))
        }
        ruby def self."enable"(_recv, stats:?, log:?, mem_size:?, call_threshold:?) {
            let _ = (&stats, &log, &mem_size, &call_threshold);
            Ok(RubyValue::Bool(false))
        }
        def self."runtime_stats" params "key = nil"(_recv, *_args) {
            Ok(RubyValue::Nil)
        }
        def self."stats_string"(_recv) {
            Ok(RubyValue::Str(crate::string_new(String::new())))
        }
        def self."exit_locations" | "log" (_recv) {
            Ok(RubyValue::Nil)
        }
        def self."dump_exit_locations" params "filename"(_recv, _path) {
            Err(crate::builtins::arg_error!(
                "--yjit-trace-exits must be enabled to use dump_exit_locations."
            ))
        }
        def self."insns_compiled" params "iseq"(_recv, _iseq) {
            Ok(RubyValue::Nil)
        }
        def self."code_gc" | "reset_stats!" | "simulate_oom!" (_recv) {
            Ok(RubyValue::Nil)
        }
        def self."disasm" params "iseq"(_recv, _what) {
            Ok(RubyValue::Nil)
        }
    }
}

// ---------------------------------------------------------------- iseq

/// An `InstructionSequence` handle: the source plus its naming -- zeo
/// compiles to native code, so this wraps "a parsed program", not bytecode.
pub(crate) struct RIseq {
    src: String,
    label: String,
    /// Whether `label` is a real one. False only for a Proc-derived `.of`
    /// handle, whose frame name zeo does not record.
    has_label: bool,
    path: String,
    absolute_path: Option<String>,
    first_lineno: i64,
    frozen: AtomicBool,
}

impl RubyObject for RIseq {
    fn class_id(&self) -> ClassId {
        RUBYVM_ISEQ_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Acquire)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Release);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = Arc::new(RIseq {
            src: self.src.clone(),
            label: self.label.clone(),
            has_label: self.has_label,
            path: self.path.clone(),
            absolute_path: self.absolute_path.clone(),
            first_lineno: self.first_lineno,
            frozen: AtomicBool::new(false),
        });
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        d
    }
}

fn recv_iseq(recv: &RubyValue) -> Arc<RIseq> {
    match recv {
        RubyValue::Object(o) => o
            .clone()
            .as_any_rc()
            .downcast::<RIseq>()
            .expect("InstructionSequence row on a non-ISeq receiver"),
        _ => unreachable!("InstructionSequence row on a non-object receiver"),
    }
}

/// The parse gate every compile row runs: a SyntaxError now, not at eval.
fn parse_check(src: &str) -> Result<(), Signal> {
    let result = ruby_prism::parse(src.as_bytes());
    match result.errors().next() {
        Some(err) => Err(crate::builtins::syntax_error!(
            "{}",
            err.message().to_string()
        )),
        None => Ok(()),
    }
}

/// The `[file, line]` a callable was written at, or `None` for one with no
/// Ruby source this runtime tracks -- a builtin row, or a `define_method`
/// body, both of which `#source_location` already reports `nil` for.
pub(crate) fn callable_source(what: &RubyValue) -> Option<(String, i64)> {
    let loc = match what {
        RubyValue::Proc(p) => {
            let (file, line) = p.location()?;
            return Some((file.to_string(), i64::from(line)));
        }
        RubyValue::Object(o) => match o
            .as_any()
            .downcast_ref::<crate::builtins::method::RMethod>()
        {
            Some(m) => crate::method_meta::source_location(
                m.meta.as_ref(),
                Some(&m.recv),
                m.home,
                m.kind,
                m.name,
            ),
            None => {
                let u = o
                    .as_any()
                    .downcast_ref::<crate::builtins::unbound_method::RUnboundMethod>()?;
                crate::method_meta::source_location(u.meta.as_ref(), None, u.home, u.kind, u.name)
            }
        },
        _ => return None,
    };
    let RubyValue::Array(a) = loc else {
        return None;
    };
    let a = a.lock();
    match (a.first(), a.get(1)) {
        (Some(RubyValue::Str(f)), Some(RubyValue::Int(l))) => {
            Some((f.lock().to_utf8_lossy().into_owned(), *l))
        }
        _ => None,
    }
}

/// A Method/UnboundMethod's iseq label, which is its own name.
fn method_label(what: &RubyValue) -> String {
    let RubyValue::Object(o) = what else {
        return String::new();
    };
    if let Some(m) = o
        .as_any()
        .downcast_ref::<crate::builtins::method::RMethod>()
    {
        return m.name.name().to_string();
    }
    match o
        .as_any()
        .downcast_ref::<crate::builtins::unbound_method::RUnboundMethod>()
    {
        Some(u) => u.name.name().to_string(),
        None => String::new(),
    }
}

/// `#label` -- the frame name, or CRuby's refusal for a handle that carries
/// none (a proc the runtime itself minted, which has no Ruby frame).
fn iseq_label(iseq: &Arc<RIseq>) -> Result<RubyValue, crate::Signal> {
    if !iseq.has_label {
        return Err(not_impl_error!(
            "RubyVM::InstructionSequence#label is not available for this Proc: \
             CRuby names the enclosing frame (`block in <main>`) and a proc the \
             runtime minted has no Ruby frame to name"
        ));
    }
    Ok(RubyValue::Str(crate::string_new(iseq.label.clone())))
}

/// `#base_label` from `#label`: the enclosing scope with the block prefix
/// `block in ` / `block (N levels) in ` removed. Anything else is its own
/// base, which is what a method handle reports.
fn base_label(label: &str) -> &str {
    if let Some(rest) = label.strip_prefix("block in ") {
        return rest;
    }
    match label.strip_prefix("block (") {
        Some(tail) => tail.split_once(") in ").map_or(label, |(_, base)| base),
        None => label,
    }
}

/// The handle `.of` answers. `label` is `None` for a Proc, whose frame name
/// zeo does not record -- `#label` then refuses rather than inventing one.
fn of_iseq_value(label: Option<String>, path: String, line: i64) -> RubyValue {
    let absolute = std::path::Path::new(&path)
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.clone());
    RubyValue::Object(Arc::new(RIseq {
        src: String::new(),
        has_label: label.is_some(),
        label: label.unwrap_or_default(),
        path,
        absolute_path: Some(absolute),
        first_lineno: line,
        frozen: AtomicBool::new(false),
    }))
}

fn iseq_value(
    src: String,
    label: String,
    path: String,
    absolute_path: Option<String>,
    first_lineno: i64,
) -> RubyValue {
    RubyValue::Object(Arc::new(RIseq {
        src,
        label,
        has_label: true,
        path,
        absolute_path,
        first_lineno,
        frozen: AtomicBool::new(false),
    }))
}

fn str_arg(v: &RubyValue) -> String {
    match v {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        other => other.to_display_string(),
    }
}

fn compile_body(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::check_arity(args.len(), 1, Some(5))?;
    let src = str_arg(&args[0]);
    parse_check(&src)?;
    let given_path = args.get(1).filter(|v| !matches!(v, RubyValue::Nil));
    let path = given_path
        .map(str_arg)
        .unwrap_or_else(|| "<compiled>".to_string());
    // The default names carry through to absolute_path; an explicit
    // RELATIVE path leaves it nil (CRuby's split).
    let absolute = args
        .get(2)
        .filter(|v| !matches!(v, RubyValue::Nil))
        .map(str_arg)
        .or_else(|| given_path.is_none().then(|| path.clone()));
    let line = match args.get(3) {
        None | Some(RubyValue::Nil) => 1,
        Some(v) => crate::builtins::arg_int!(v),
    };
    Ok(iseq_value(src, path.clone(), path, absolute, line))
}

const NO_YARV: &str =
    "zeo compiles ahead of time to native code; there is no YARV bytecode to serialize";

mod iseq {
    use super::*;

    ruby_class! {
        InstructionSequence = zeo_abi::RUBYVM_ISEQ_CLASS < zeo_abi::OBJECT_CLASS;

        def self."compile" | "new" | "compile_prism" | "compile_parsey" (_recv, *args) {
            compile_body(args)
        }
        def self."compile_file" | "compile_file_prism" cfunc (_recv, path, _opt?) {
            let path = str_arg(path);
            let src = std::fs::read_to_string(&path)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "read", &path))?;
            parse_check(&src)?;
            Ok(iseq_value(src, "<main>".to_string(), path.clone(), Some(path), 1))
        }
        def self."compile_option"(_recv) {
            Ok(COMPILE_OPTION
                .get_or_init(|| PlMutex::new(RubyValue::Hash(crate::hash_new(vec![]))))
                .lock()
                .clone())
        }
        def self."compile_option="(_recv, value) {
            *COMPILE_OPTION
                .get_or_init(|| PlMutex::new(RubyValue::Hash(crate::hash_new(vec![]))))
                .lock() = value.clone();
            Ok(value.clone())
        }
        // An iseq for a callable written in Ruby, `nil` for a C-defined one
        // -- which is the question callers actually ask of this row, and irb's
        // source finder leans on `.of(m)&.script_lines`.
        //
        // The handle carries no bytecode (there is none), so `#to_a`,
        // `#to_binary` and `#disasm` refuse on it exactly as they do on a
        // compiled one. `#label` on a PROC-derived handle refuses too: CRuby
        // answers the enclosing frame's name (`block in <main>`) and a zeo
        // Proc carries no label, only a location.
        def self."of"(_recv, what) {
            let Some((path, line)) = callable_source(what) else {
                return Ok(RubyValue::Nil);
            };
            let label = match what {
                // A block's frame label is a LEXICAL fact, so codegen stamps
                // it on the shape and the Proc carries it here. A proc the
                // runtime itself minted has no Ruby frame to name.
                RubyValue::Proc(p) => p.frame_label().map(str::to_string),
                _ => Some(method_label(what)),
            };
            Ok(of_iseq_value(label, path, line))
        }
        def self."disasm" | "disassemble" (_recv, _what) {
            Err(not_impl_error!("{}", NO_YARV))
        }
        def self."load_from_binary" | "load_from_binary_extra_data" (_recv, _data) {
            Err(crate::builtins::runtime_error!("broken binary format"))
        }

        def "eval"(recv) {
            let iseq = recv_iseq(recv);
            crate::eval_string(&iseq.src, crate::dispatch::main_object(), 0)
        }
        def "label"(recv) {
            let iseq = recv_iseq(recv);
            iseq_label(&iseq)
        }
        // The enclosing SCOPE's name: a block reports the method (or
        // `<main>`) it was written in, however deeply it nests. Ruby builds
        // both from one frame, so zeo strips what `block_label` added.
        def "base_label"(recv) {
            let iseq = recv_iseq(recv);
            let full = iseq_label(&iseq)?;
            let RubyValue::Str(s) = &full else { return Ok(full) };
            let text = s.lock().to_utf8_lossy().into_owned();
            Ok(RubyValue::Str(crate::string_new(base_label(&text).to_string())))
        }
        def "path"(recv) {
            Ok(RubyValue::Str(crate::string_new(recv_iseq(recv).path.clone())))
        }
        def "absolute_path"(recv) {
            Ok(match &recv_iseq(recv).absolute_path {
                Some(p) => RubyValue::Str(crate::string_new(p.clone())),
                None => RubyValue::Nil,
            })
        }
        def "first_lineno"(recv) {
            Ok(RubyValue::Int(recv_iseq(recv).first_lineno))
        }
        def "script_lines"(_recv) {
            Ok(RubyValue::Nil)
        }
        def "trace_points"(recv) {
            let iseq = recv_iseq(recv);
            Ok(RubyValue::Array(crate::array_new(vec![RubyValue::Array(
                crate::array_new(vec![
                    RubyValue::Int(iseq.first_lineno),
                    RubyValue::Symbol(crate::Symbol::intern("line")),
                ]),
            )])))
        }
        def "each_child"(recv, &_block) {
            // A source-wrapping iseq holds no compiled children.
            Ok(recv.clone())
        }
        def "inspect"(recv) {
            let iseq = recv_iseq(recv);
            Ok(RubyValue::Str(crate::string_new(format!(
                "<RubyVM::InstructionSequence:{}@{}:{}>",
                iseq.label, iseq.path, iseq.first_lineno
            ))))
        }
        def "to_a" | "disasm" | "disassemble" (_recv) {
            Err(not_impl_error!("{}", NO_YARV))
        }
        def "to_binary"(_recv, *_args) {
            Err(not_impl_error!("{}", NO_YARV))
        }
    }
}

static COMPILE_OPTION: std::sync::OnceLock<PlMutex<RubyValue>> = std::sync::OnceLock::new();

// The compile rows parse-check and mint a handle; nothing needs a live
// program, so they are honest unit-test material. `#eval`/`.of` go through
// the real compiler seam and stay with the e2e suites.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Symbol;

    // Each test runs in its own nextest process -- see the crate README for
    // the with_core() bootstrap pattern.
    fn install_core() {
        crate::dispatch::install_class_registry(crate::dispatch::ClassRegistry::with_core());
    }

    fn call(recv: &RubyValue, name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
        crate::dispatch::send_value(recv, Symbol::intern(name), args, None)
    }

    fn compile(src: &str) -> Result<RubyValue, Signal> {
        call(
            &RubyValue::Class(zeo_abi::RUBYVM_ISEQ_CLASS),
            "compile",
            &[RubyValue::Str(crate::string_new(src.to_string()))],
        )
    }

    #[test]
    fn compile_answers_a_handle_with_the_default_names() {
        install_core();
        let iseq = compile("1 + 1").expect("a valid snippet compiles");
        assert!(matches!(
            call(&iseq, "label", &[]),
            Ok(RubyValue::Str(s)) if s.lock().to_utf8_lossy() == "<compiled>"
        ));
        assert!(matches!(
            call(&iseq, "path", &[]),
            Ok(RubyValue::Str(s)) if s.lock().to_utf8_lossy() == "<compiled>"
        ));
        assert!(matches!(
            call(&iseq, "first_lineno", &[]),
            Ok(RubyValue::Int(1))
        ));
    }

    #[test]
    fn a_parse_error_raises_syntax_error_at_compile_time() {
        install_core();
        let err = compile("def");
        let Err(Signal::Raise(exc)) = err else {
            panic!("expected a SyntaxError raise");
        };
        assert_eq!(
            exc.as_object_unchecked().class_id(),
            zeo_abi::SYNTAX_ERROR_CLASS
        );
    }
}
