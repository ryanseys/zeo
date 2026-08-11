//! Runtime string `eval` -- a tree-walking interpreter over
//! `ruby-prism`'s own `Node` tree, linked into the runtime behind the
//! `eval-vm` feature.
//!
//! **Why walk prism directly, not the compiler's HIR?** The HIR bakes in
//! decisions the AOT compiler resolves statically -- `New` needs a
//! statically-known class, `ClassRef` has no first-class runtime `Class`
//! value, `super` lowers to inlining the parent body, class-var owners are
//! pre-resolved in `analyze`. An interpreter would have to *undo* all of
//! that. Prism's `Node` tree, by contrast, IS Ruby's surface semantics, and
//! every construct maps onto a primitive the runtime already exposes:
//! `dispatch::send_value` for method calls, `const_get`/`global_get`/
//! `ivar_*`/`cvar_*` for state, the `runtime_meta` overlay for runtime
//! `def`/`class`. So the eval VM is a purely additive module against the
//! live runtime, not a second compiler.
//!
//! **Scope.** An `eval` runs with a correct `self` (its receiver's ivars,
//! implicit-self calls, constants, globals) and can `def` methods (installed
//! on the mode's default definee -- an instance method for `class_eval`, a
//! singleton for `instance_eval`, `self`'s class for a plain `eval`), run
//! blocks passed to calls, and `yield`/`return` inside an eval-defined
//! method; each such method/block re-parses its own captured source per
//! invocation. The CALLER's own locals come along too: a compiled `eval` site
//! materializes its scope as a `Binding` (see `builtins::binding`), whose
//! cells ARE this interpreter's `Env::scope`, so the source reads and writes
//! them. Without an explicit binding that scope is a CHILD of the caller's, so
//! a local the source introduces dies with the call -- `Binding#eval` runs in
//! the Binding itself and keeps it.

// Feature-split imports: the interpreter (`mod imp`, eval-vm on) raises
// NameError for unresolved constants; the feature-off stub raises
// NotImplementedError. Each import exists only where its arm compiles, or
// the other build flags it unused.
use crate::builtins::binding::RBinding;
#[cfg(feature = "eval-vm")]
use crate::builtins::name_error;
#[cfg(not(feature = "eval-vm"))]
use crate::builtins::not_impl_error;
use crate::{RubyValue, Signal};

/// Which surface invoked the eval -- it decides where a `def` inside the
/// source installs (the "default definee", CRuby's `cref`):
/// - `Caller` (`Kernel#eval`): an instance method on `self`'s class (a plain
///   top-level eval's `self` is the main object, so `def` lands on `Object`).
/// - `ClassEval` (`Module#class_eval`): an instance method on `self` (a Class).
/// - `InstanceEval` (`BasicObject#instance_eval`): a singleton method on
///   `self` (a class method when `self` is itself a Class).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EvalMode {
    Caller,
    ClassEval,
    InstanceEval,
}

/// Evaluate `src` as a standalone chunk of Ruby with `self` bound to
/// `self_val`, resolving constants/globals against `box_id`. A parse failure
/// becomes a catchable `SyntaxError`; the value of the last statement is
/// returned (`nil` for an empty program). `mode` decides where a `def`
/// inside the source installs -- see [`EvalMode`].
///
/// Compiled without the `eval-vm` feature this is the honest stub: a
/// `NotImplementedError` naming the missing feature, mirroring the runtime's
/// other "not compiled in" surfaces.
pub fn eval_string_mode(
    src: &str,
    self_val: RubyValue,
    box_id: u32,
    mode: EvalMode,
) -> Result<RubyValue, Signal> {
    #[cfg(feature = "eval-vm")]
    {
        imp::eval_string(src, self_val, box_id, mode)
    }
    #[cfg(not(feature = "eval-vm"))]
    {
        let _ = (src, self_val, box_id, mode);
        Err(not_impl_error!(
            "string eval requires the eval VM (build zeo-rt with --features eval-vm)"
        ))
    }
}

/// [`eval_string_mode`] in the default `Kernel#eval` mode -- the entry the
/// literal-splice fallback and the internal unit tests use.
pub fn eval_string(src: &str, self_val: RubyValue, box_id: u32) -> Result<RubyValue, Signal> {
    eval_string_mode(src, self_val, box_id, EvalMode::Caller)
}

/// The dynamic-`eval` entry every dispatch site funnels through: coerce the
/// source argument to a String (Ruby raises `TypeError` for anything else,
/// even a Symbol) and evaluate it with `self` bound to `self_val`. Keeping the
/// coercion here means every caller -- `Kernel#eval`, `instance_eval`,
/// `class_eval` -- shares one definition of "what counts as evalable source".
pub fn eval_value_mode(
    src: RubyValue,
    self_val: RubyValue,
    box_id: u32,
    mode: EvalMode,
) -> Result<RubyValue, Signal> {
    let code = crate::builtins::convert::to_rstr(&src)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    eval_string_mode(&code, self_val, box_id, mode)
}

/// [`eval_value_mode`] in the default `Kernel#eval` mode.
pub fn eval_value(src: RubyValue, self_val: RubyValue, box_id: u32) -> Result<RubyValue, Signal> {
    eval_value_mode(src, self_val, box_id, EvalMode::Caller)
}

/// The compiled receiver-less `eval(src[, binding[, file[, line]]])` site.
/// CRuby runs a bare `eval` -- and one given a `nil` binding -- in the
/// CALLER's own frame, so codegen hands that frame over as `scope`,
/// materialized right at the call site (`codegen::call::emit_binding_value`);
/// that is what lets the source read and write the caller's locals. An
/// explicit non-nil binding wins over it, and must be a `Binding`.
pub fn eval_value_in_scope(
    src: RubyValue,
    scope: RubyValue,
    binding: RubyValue,
    file: RubyValue,
    line: RubyValue,
) -> Result<RubyValue, Signal> {
    let chosen = if binding.is_nil() { &scope } else { &binding };
    let Some(b) = crate::builtins::binding::as_binding(chosen) else {
        return Err(crate::builtins::type_error!(
            "wrong argument type {} (expected binding)",
            crate::dispatch::class_name(binding.class_id()).unwrap_or_else(|| "Object".to_string())
        ));
    };
    // Without an explicit binding the source runs in a CHILD of the caller's
    // frame: it reads and writes the caller's locals, but a name it introduces
    // is its own and dies with the call. `Binding#eval` keeps them, because
    // there the Binding IS the scope.
    let implicit;
    let b = if binding.is_nil() {
        implicit = RBinding {
            self_val: b.self_val.clone(),
            scope: b.scope.child(),
            file: b.file.clone(),
            line: b.line,
            box_id: b.box_id,
            cref: b.cref,
            frozen: std::sync::atomic::AtomicBool::new(false),
        };
        &implicit
    } else {
        b
    };
    let file = match &file {
        RubyValue::Nil => None,
        v => Some(
            crate::builtins::convert::to_rstr(v)?
                .lock()
                .to_utf8_lossy()
                .into_owned(),
        ),
    };
    let line = match &line {
        RubyValue::Nil => None,
        v => Some(crate::builtins::convert::to_index(v)? as u32),
    };
    eval_with_binding(&src, b, file, line)
}

/// `Binding#eval` and `Kernel#eval(src, binding, ...)` -- the source runs in
/// the captured scope: `b`'s locals ARE the eval's locals (shared cells, so a
/// write reaches the compiled frame), `b`'s `self` is the receiver, and `b`'s
/// cref is what a constant resolves against. `file`/`line` override what
/// `__FILE__`/`__LINE__` report, as CRuby's own 3rd/4th `eval` arguments do.
pub fn eval_with_binding(
    src: &RubyValue,
    b: &RBinding,
    file: Option<String>,
    line: Option<u32>,
) -> Result<RubyValue, Signal> {
    let code = crate::builtins::convert::to_rstr(src)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    #[cfg(feature = "eval-vm")]
    {
        imp::eval_in_binding(&code, b, file, line)
    }
    #[cfg(not(feature = "eval-vm"))]
    {
        let _ = (code, b, file, line);
        Err(not_impl_error!(
            "string eval requires the eval VM (build zeo-rt with --features eval-vm)"
        ))
    }
}

#[cfg(feature = "eval-vm")]
mod imp {
    use super::*;
    use crate::builtins::binding::BindingScope;
    use crate::builtins::{arg_error, type_error};
    use crate::{RProc, Symbol};
    use ruby_prism::{DefNode, Node, NodeList, StatementsNode};
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Where a `def` in the current lexical context installs -- CRuby's
    /// default definee (`cref`). A `class`/`module` body pushes a fresh
    /// `Class` definee; `def self.x` / `def obj.x` targets a `Singleton`.
    #[derive(Clone)]
    enum Definee {
        /// An instance method on this class (top-level `Object`, a class body,
        /// a `class_eval`).
        Class(crate::ClassId),
        /// A singleton method on this value (`instance_eval`, `def self.x`).
        Singleton(RubyValue),
    }

    /// The lexical environment a single `eval` runs in -- see the module docs
    /// on the "top-level-first" scope decision.
    struct Env {
        /// The `self` every implicit-receiver call and `@ivar` access binds to.
        self_val: RubyValue,
        /// This scope's local variables. A plain eval starts with an empty
        /// scope of its own; an eval THROUGH a `Binding` runs directly in that
        /// Binding's, so the caller's compiled locals are readable and a write
        /// reaches the frame's own cell. A local read prism resolved (it was
        /// assigned earlier in the eval source) but which no executed statement
        /// has written yet reads as `nil`, exactly as a declared-but-unassigned
        /// Ruby local does.
        scope: Arc<BindingScope>,
        /// Defining box for constant/global resolution.
        box_id: u32,
        /// Where a bare `def` installs (see [`Definee`]).
        definee: Definee,
        /// The block available to a `yield` in the current method body -- set
        /// when interpreting an eval-defined method, `None` at eval top level.
        block: Option<RubyValue>,
        /// The positional args the current eval-defined method was called with
        /// -- what a bare `super` (zsuper) forwards. `None` at eval top level.
        method_args: Option<Vec<RubyValue>>,
        /// The full source under interpretation, shared so a `def` can slice
        /// out its own text to re-parse on each later invocation (the eval VM
        /// holds no `'src`-lifetime nodes past the call that built them).
        src: Arc<str>,
        /// The lexical class a constant read resolves against before the top
        /// level -- a `Binding`'s captured cref, or the RECEIVER for the
        /// string form of `class_eval` (its whole point: the receiver is the
        /// scope). `None` for a plain `Caller` eval, which keeps the
        /// documented top-level-first rule.
        cref: Option<crate::ClassId>,
        /// How a constant MISS qualifies its NameError -- the receiver's
        /// name for `class_eval` (`Outer::Host::VAL`), the singleton
        /// spelling for `instance_eval` on a class
        /// (`#<Class:Outer::Host>::HOST_C`). `None` = the bare top-level
        /// message.
        cref_name: Option<String>,
        /// What `__FILE__`/`__LINE__` report, and where a `binding` taken
        /// inside this eval says it came from -- `eval`'s own 3rd/4th
        /// arguments when given, `("(eval)", 1)` otherwise.
        file: Arc<str>,
        line: u32,
    }

    /// The environment a fresh (non-`Binding`) eval scope starts from: no
    /// locals, no cref, `(eval)` as its source.
    fn fresh_scope() -> Arc<BindingScope> {
        Arc::new(BindingScope::new(Vec::new()))
    }

    const EVAL_FILE: &str = "(eval)";

    pub(super) fn eval_string(
        src: &str,
        self_val: RubyValue,
        box_id: u32,
        mode: EvalMode,
    ) -> Result<RubyValue, Signal> {
        let result = ruby_prism::parse(src.as_bytes());
        if let Some(err) = result.errors().next() {
            return Err(crate::dispatch::raise_error(
                "SyntaxError",
                err.message().to_string(),
            ));
        }
        let program = result
            .node()
            .as_program_node()
            .ok_or_else(|| internal("eval: expected a top-level ProgramNode"))?;
        let definee = if box_id != 0 && matches!(mode, EvalMode::Caller) {
            // A boxed eval's top level IS the box: bare constant writes and
            // `class Foo` land on the surrogate, not on `Object`.
            Definee::Class(crate::ClassId(crate::boxes::surrogate_of(box_id)))
        } else {
            initial_definee(&self_val, mode)
        };
        // The string forms of class_eval/instance_eval evaluate with the
        // RECEIVER as the constant scope (the block forms keep the writer's
        // lexical scope and never come here): class_eval resolves against
        // the class itself; instance_eval on a class resolves against its
        // SINGLETON (which owns no constants -- the miss is the answer, in
        // the #<Class:X> spelling).
        let (cref, cref_name) = match (&mode, &self_val) {
            (EvalMode::ClassEval, RubyValue::Class(cid)) => {
                (Some(*cid), crate::dispatch::class_name(*cid))
            }
            (EvalMode::InstanceEval, RubyValue::Class(cid)) => (
                None,
                crate::dispatch::class_name(*cid).map(|n| format!("#<Class:{n}>")),
            ),
            _ => (None, None),
        };
        let mut env = Env {
            self_val,
            scope: fresh_scope(),
            box_id,
            definee,
            block: None,
            method_args: None,
            src: Arc::from(src),
            cref,
            cref_name,
            file: Arc::from(EVAL_FILE),
            line: 1,
        };
        eval_list(&program.statements().body(), &mut env)
    }

    /// [`super::eval_with_binding`]'s interpreter half -- the same walk, run
    /// against the captured scope instead of a fresh one.
    pub(super) fn eval_in_binding(
        src: &str,
        b: &RBinding,
        file: Option<String>,
        line: Option<u32>,
    ) -> Result<RubyValue, Signal> {
        let result = ruby_prism::parse(src.as_bytes());
        if let Some(err) = result.errors().next() {
            return Err(crate::dispatch::raise_error(
                "SyntaxError",
                err.message().to_string(),
            ));
        }
        let program = result
            .node()
            .as_program_node()
            .ok_or_else(|| internal("eval: expected a top-level ProgramNode"))?;
        let mut env = Env {
            self_val: b.self_val.clone(),
            scope: Arc::clone(&b.scope),
            box_id: b.box_id,
            definee: initial_definee(&b.self_val, EvalMode::Caller),
            block: None,
            method_args: None,
            src: Arc::from(src),
            cref: b.cref,
            cref_name: None,
            file: Arc::from(file.as_deref().unwrap_or(EVAL_FILE)),
            line: line.unwrap_or(1),
        };
        eval_list(&program.statements().body(), &mut env)
    }

    /// The `cref` an eval starts with, from its mode and `self` -- see
    /// [`EvalMode`].
    fn initial_definee(self_val: &RubyValue, mode: EvalMode) -> Definee {
        match mode {
            EvalMode::InstanceEval => Definee::Singleton(self_val.clone()),
            // A class body context (`class_eval`, or a `Caller` eval whose
            // `self` is already a Class) installs instance methods on it; a
            // plain object's `Caller` eval installs on that object's class
            // (top-level `self` is main -> `Object`).
            EvalMode::ClassEval | EvalMode::Caller => match self_val {
                RubyValue::Class(cid) => Definee::Class(*cid),
                other => Definee::Class(other.class_id()),
            },
        }
    }

    /// Evaluate every node in a list, answering the last value (`nil` if empty).
    fn eval_list(body: &NodeList<'_>, env: &mut Env) -> Result<RubyValue, Signal> {
        let mut last = RubyValue::Nil;
        for node in body.iter() {
            last = eval_node(&node, env)?;
        }
        Ok(last)
    }

    /// An optional `StatementsNode` (the body of a `def`/`if`/`while`/...):
    /// `None` answers `nil`, matching Ruby's empty body.
    fn eval_opt_stmts(
        stmts: Option<StatementsNode<'_>>,
        env: &mut Env,
    ) -> Result<RubyValue, Signal> {
        match stmts {
            Some(s) => eval_list(&s.body(), env),
            None => Ok(RubyValue::Nil),
        }
    }

    fn eval_node(node: &Node<'_>, env: &mut Env) -> Result<RubyValue, Signal> {
        // The interpreter recurses on the native stack too.
        crate::stack_guard::stack_check()?;
        // ---- literals -------------------------------------------------------
        if let Some(int) = node.as_integer_node() {
            // prism's own arbitrary-precision value (LSB-first u32 digits);
            // `int_from_u32_digits` demotes to `Int` whenever it fits.
            let value = int.value();
            let (negative, digits) = value.to_u32_digits();
            return Ok(crate::int_from_u32_digits(negative, digits));
        }
        if let Some(float) = node.as_float_node() {
            return Ok(RubyValue::Float(float.value()));
        }
        if let Some(s) = node.as_string_node() {
            let text = String::from_utf8_lossy(s.unescaped()).into_owned();
            return Ok(RubyValue::Str(crate::string_new(text)));
        }
        if let Some(sym) = node.as_symbol_node() {
            let name = String::from_utf8_lossy(sym.unescaped()).into_owned();
            return Ok(RubyValue::Symbol(crate::Symbol::intern(&name)));
        }
        if node.as_nil_node().is_some() {
            return Ok(RubyValue::Nil);
        }
        if node.as_true_node().is_some() {
            return Ok(RubyValue::Bool(true));
        }
        if node.as_false_node().is_some() {
            return Ok(RubyValue::Bool(false));
        }
        if node.as_self_node().is_some() {
            return Ok(env.self_val.clone());
        }
        if node.as_source_file_node().is_some() {
            return Ok(RubyValue::Str(crate::string_new(env.file.to_string())));
        }
        if node.as_source_line_node().is_some() {
            // `eval`'s `lineno` argument numbers the source's FIRST line, so a
            // `__LINE__` further in counts newlines from there.
            let upto = node.location().start_offset().min(env.src.len());
            let within = env.src.as_bytes()[..upto]
                .iter()
                .filter(|&&b| b == b'\n')
                .count() as u32;
            return Ok(RubyValue::Int((env.line + within) as i64));
        }
        if let Some(istr) = node.as_interpolated_string_node() {
            return eval_interpolated(&istr.parts(), env);
        }

        // ---- locals ---------------------------------------------------------
        if let Some(lvr) = node.as_local_variable_read_node() {
            let name = String::from_utf8_lossy(lvr.name().as_slice()).into_owned();
            return Ok(env.scope.get(&name).unwrap_or(RubyValue::Nil));
        }
        if let Some(lvw) = node.as_local_variable_write_node() {
            let name = String::from_utf8_lossy(lvw.name().as_slice()).into_owned();
            let value = eval_node(&lvw.value(), env)?;
            env.scope.set(&name, value.clone());
            return Ok(value);
        }

        // ---- instance variables (against `self`) ----------------------------
        // The runtime keys ivars by their DE-`@`'d name (a Rust struct field
        // can't be spelled `@x`), the same convention `instance_variable_get`
        // uses -- prism hands us the name WITH the sigil, so strip it.
        if let Some(ivr) = node.as_instance_variable_read_node() {
            let name = ivar_key(ivr.name().as_slice());
            return Ok(crate::ivar_get_dyn(&env.self_val, &name));
        }
        if let Some(ivw) = node.as_instance_variable_write_node() {
            let name = ivar_key(ivw.name().as_slice());
            let value = eval_node(&ivw.value(), env)?;
            crate::ivar_set_dyn(&env.self_val, &name, value.clone())?;
            return Ok(value);
        }

        // ---- globals --------------------------------------------------------
        // Keyed by the full `$name` (prism includes the sigil, and so does the
        // runtime's own global table).
        if let Some(gvr) = node.as_global_variable_read_node() {
            let name = String::from_utf8_lossy(gvr.name().as_slice()).into_owned();
            return Ok(crate::global_get(env.box_id, &name));
        }
        if let Some(gvw) = node.as_global_variable_write_node() {
            let name = String::from_utf8_lossy(gvw.name().as_slice()).into_owned();
            let value = eval_node(&gvw.value(), env)?;
            crate::global_set(env.box_id, &name, value.clone());
            return Ok(value);
        }

        // ---- constants ------------------------------------------------------
        if let Some(cr) = node.as_constant_read_node() {
            let name = String::from_utf8_lossy(cr.name().as_slice()).into_owned();
            // A `Binding`'s cref first (`binding.eval("K")` inside `module M`
            // finds `M::K`), then the root -- an eval'd chunk with no captured
            // lexical scope resolves against `Object`, class id 0.
            if let Some(cref) = env.cref
                && let Some(v) = crate::constants::const_get(cref.0, &name)
            {
                return Ok(v);
            }
            // A scoped eval's miss names its scope -- `Outer::Host::VAL` for
            // class_eval, `#<Class:Outer::Host>::HOST_C` for instance_eval
            // on a class (CRuby's spellings).
            if let Some(scope) = &env.cref_name
                && crate::constants::const_get(0, &name).is_none()
            {
                return Err(name_error!("uninitialized constant {scope}::{name}"));
            }
            // Boxed code's top level is its surrogate, then the MASTER
            // namespace -- never main's own mutations (the isolation line).
            if env.box_id != 0 {
                let top = crate::boxes::surrogate_of(env.box_id);
                if let Some(v) = crate::constants::const_get(top, &name) {
                    return Ok(v);
                }
                if let Some(v) = crate::constants::const_get_master(&name) {
                    return Ok(v);
                }
                return Err(name_error!("uninitialized constant {name}"));
            }
            return const_lookup(0, &name);
        }
        if let Some(cp) = node.as_constant_path_node() {
            // `Foo::Bar` / `::Foo` -- resolve the parent to a class/module VALUE,
            // then look the name up in its own constant table. A leading `::`
            // (no parent) anchors at the top level.
            let name = cp
                .name()
                .map(|n| String::from_utf8_lossy(n.as_slice()).into_owned())
                .ok_or_else(|| internal("eval: a computed `::` constant name is not supported"))?;
            let owner = match cp.parent() {
                None => 0,
                Some(parent) => match eval_node(&parent, env)? {
                    RubyValue::Class(cid) => cid.0,
                    other => {
                        return Err(type_error!(
                            "{} is not a class/module",
                            other.inspect_string()
                        ));
                    }
                },
            };
            return const_lookup_scoped(owner, &name);
        }

        // ---- control flow ---------------------------------------------------
        if let Some(if_node) = node.as_if_node() {
            let cond = eval_node(&if_node.predicate(), env)?;
            return if truthy(&cond) {
                eval_opt_stmts(if_node.statements(), env)
            } else {
                match if_node.subsequent() {
                    Some(sub) => eval_node(&sub, env),
                    None => Ok(RubyValue::Nil),
                }
            };
        }
        if let Some(unless_node) = node.as_unless_node() {
            let cond = eval_node(&unless_node.predicate(), env)?;
            return if truthy(&cond) {
                match unless_node.else_clause() {
                    Some(e) => eval_opt_stmts(e.statements(), env),
                    None => Ok(RubyValue::Nil),
                }
            } else {
                eval_opt_stmts(unless_node.statements(), env)
            };
        }
        if node.as_else_node().is_some() {
            // Reached only as an `IfNode::subsequent()` -- the final `else`.
            let e = node.as_else_node().unwrap();
            return eval_opt_stmts(e.statements(), env);
        }
        // `case`/`when`, both forms: with a subject each `when` value asks
        // `value === subject` (a real dispatch -- Class#===, Regexp#===,
        // custom overrides); the subjectless form takes the first truthy
        // condition. minitest's `infect_an_assertion` template (the body
        // behind every `must_*` expectation) is a subjectless case, which is
        // what put this node in reach of eval'd code at all.
        if let Some(case_node) = node.as_case_node() {
            let subject = match case_node.predicate() {
                Some(p) => Some(eval_node(&p, env)?),
                None => None,
            };
            for cond in case_node.conditions().iter() {
                let Some(when) = cond.as_when_node() else {
                    return Err(internal(
                        "eval: unsupported clause in a case expression",
                    ));
                };
                for c in when.conditions().iter() {
                    let v = eval_node(&c, env)?;
                    let hit = match &subject {
                        Some(s) => {
                            let answer = crate::dispatch::send_value(
                                &v,
                                Symbol::intern("==="),
                                std::slice::from_ref(s),
                                None,
                            )?;
                            truthy(&answer)
                        }
                        None => truthy(&v),
                    };
                    if hit {
                        return eval_opt_stmts(when.statements(), env);
                    }
                }
            }
            return match case_node.else_clause() {
                Some(e) => eval_opt_stmts(e.statements(), env),
                None => Ok(RubyValue::Nil),
            };
        }
        if let Some(while_node) = node.as_while_node() {
            while {
                let c = eval_node(&while_node.predicate(), env)?;
                truthy(&c)
            } {
                eval_opt_stmts(while_node.statements(), env)?;
            }
            return Ok(RubyValue::Nil);
        }
        if let Some(until_node) = node.as_until_node() {
            while {
                let c = eval_node(&until_node.predicate(), env)?;
                !truthy(&c)
            } {
                eval_opt_stmts(until_node.statements(), env)?;
            }
            return Ok(RubyValue::Nil);
        }
        if let Some(and_node) = node.as_and_node() {
            let left = eval_node(&and_node.left(), env)?;
            return if truthy(&left) {
                eval_node(&and_node.right(), env)
            } else {
                Ok(left)
            };
        }
        if let Some(or_node) = node.as_or_node() {
            let left = eval_node(&or_node.left(), env)?;
            return if truthy(&left) {
                Ok(left)
            } else {
                eval_node(&or_node.right(), env)
            };
        }

        // ---- grouping -------------------------------------------------------
        if let Some(paren) = node.as_parentheses_node() {
            return match paren.body() {
                Some(b) => eval_node(&b, env),
                None => Ok(RubyValue::Nil),
            };
        }
        if let Some(stmts) = node.as_statements_node() {
            return eval_list(&stmts.body(), env);
        }

        // ---- collections ----------------------------------------------------
        if let Some(array) = node.as_array_node() {
            let mut elems = Vec::new();
            for el in array.elements().iter() {
                if let Some(splat) = el.as_splat_node() {
                    if let Some(expr) = splat.expression() {
                        splat_into(&mut elems, eval_node(&expr, env)?);
                    }
                } else {
                    elems.push(eval_node(&el, env)?);
                }
            }
            return Ok(RubyValue::Array(crate::array_new(elems)));
        }
        if let Some(hash) = node.as_hash_node() {
            let mut pairs = Vec::new();
            for el in hash.elements().iter() {
                let assoc = el.as_assoc_node().ok_or_else(|| {
                    internal("eval: `**` double-splat in a hash literal is not supported yet")
                })?;
                let key = eval_node(&assoc.key(), env)?;
                let value = eval_node(&assoc.value(), env)?;
                pairs.push((key, value));
            }
            return Ok(RubyValue::Hash(crate::hash_new(pairs)));
        }
        if let Some(range) = node.as_range_node() {
            let start = match range.left() {
                Some(n) => Some(Box::new(eval_node(&n, env)?)),
                None => None,
            };
            let end = match range.right() {
                Some(n) => Some(Box::new(eval_node(&n, env)?)),
                None => None,
            };
            return Ok(RubyValue::Range(start, end, range.is_exclude_end()));
        }

        // ---- def / class / module -------------------------------------------
        if let Some(def) = node.as_def_node() {
            return eval_def(&def, env);
        }
        if let Some(class) = node.as_class_node() {
            return eval_class_or_module(
                &class.constant_path(),
                class.superclass(),
                class.body(),
                false,
                env,
            );
        }
        if let Some(module) = node.as_module_node() {
            return eval_class_or_module(&module.constant_path(), None, module.body(), true, env);
        }

        // ---- return / yield (inside an eval-defined method body) ------------
        if let Some(ret) = node.as_return_node() {
            let value = match ret.arguments() {
                Some(a) => eval_arguments_single(&a, env)?,
                None => RubyValue::Nil,
            };
            return Err(Signal::Return(value));
        }
        if let Some(y) = node.as_yield_node() {
            let Some(block) = env.block.clone() else {
                return Err(crate::builtins::local_jump_error!("no block given (yield)"));
            };
            let mut args = Vec::new();
            if let Some(a) = y.arguments() {
                collect_arguments(&a.arguments(), &mut args, env)?;
            }
            return crate::dispatch::send_value_in(
                env.box_id,
                &block,
                Symbol::intern("call"),
                &args,
                None,
            );
        }

        // ---- super (inside an eval-defined method) --------------------------
        // The method was installed via `runtime_define_method`, whose wrapper
        // pushes the (defining class, name) frame `send_super_dynamic` reads.
        if let Some(sup) = node.as_super_node() {
            let mut args = Vec::new();
            if let Some(a) = sup.arguments() {
                collect_arguments(&a.arguments(), &mut args, env)?;
            }
            return crate::runtime_meta::send_super_dynamic(
                &env.self_val,
                &args,
                env.block.clone(),
            );
        }
        if node.as_forwarding_super_node().is_some() {
            // Bare `super` forwards the method's own arguments.
            let args = env.method_args.clone().unwrap_or_default();
            return crate::runtime_meta::send_super_dynamic(
                &env.self_val,
                &args,
                env.block.clone(),
            );
        }

        // `defined?(expr)` -- forwardable's generated delegators guard with
        // `if defined?(_.method)`. Only the shapes that guard are classified:
        // a method call is probed for real (evaluating the receiver, as Ruby
        // does), a local/ivar/global/constant answers by whether it is set, and
        // anything else answers "expression" -- CRuby's catch-all for a plain
        // value, which is what an unrecognized operand reduces to here.
        if let Some(defined) = node.as_defined_node() {
            return eval_defined(&defined.value(), env);
        }

        // ---- method calls ---------------------------------------------------
        if let Some(call) = node.as_call_node() {
            return eval_call(&call, env);
        }

        Err(internal(
            "eval: unsupported syntax in this build (the eval VM does not yet cover this node)",
        ))
    }

    /// `def name(params) body end` / `def self.name ...` / `def recv.name ...`
    /// -- install a method whose body is INTERPRETED by re-parsing its own
    /// source on each call (the VM keeps no `'src` nodes past this point).
    /// Installs per the current [`Definee`], or on the receiver's singleton
    /// for a `def recv.x`. Returns the method name Symbol, as Ruby's `def`
    /// expression does.
    fn eval_def(def: &DefNode<'_>, env: &mut Env) -> Result<RubyValue, Signal> {
        let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        let sym = Symbol::intern(&name);
        let target = match def.receiver() {
            Some(r) => Definee::Singleton(eval_node(&r, env)?),
            None => env.definee.clone(),
        };
        let body = make_eval_method(&env.src, def, env.box_id)?;
        match target {
            Definee::Class(cid) => crate::runtime_define_method(cid, sym, body)?,
            Definee::Singleton(val) => {
                crate::runtime_meta::runtime_define_singleton_method(&val, sym, body)?
            }
        };
        Ok(RubyValue::Symbol(sym))
    }

    /// `class Name [< Super] ... end` / `module Name ... end` inside eval.
    /// Reopens an existing class/module (a compiled AOT one, or one an earlier
    /// eval created) or mints a fresh runtime one, then interprets the body
    /// with `self` and the definee bound to it. The body runs in a FRESH local
    /// scope (a Ruby class body sees no enclosing locals). Returns the body's
    /// last value.
    fn eval_class_or_module(
        cpath: &Node<'_>,
        superclass: Option<Node<'_>>,
        body: Option<Node<'_>>,
        is_module: bool,
        env: &mut Env,
    ) -> Result<RubyValue, Signal> {
        let (owner, name) = cpath_target(cpath, env)?;
        let existing = match crate::constants::const_get(owner, &name) {
            Some(RubyValue::Class(cid)) => Some(cid),
            _ if owner == 0 => crate::dispatch::class_id_by_name(&name),
            _ => None,
        };
        let class_id = match existing {
            Some(cid) => cid,
            None => {
                let super_val = match superclass {
                    Some(s) => Some(eval_node(&s, env)?),
                    None => None,
                };
                let val = if is_module {
                    crate::runtime_meta::runtime_module_new(None)?
                } else {
                    crate::runtime_meta::runtime_class_new(super_val, None)?
                };
                let RubyValue::Class(cid) = val else {
                    return Err(internal(
                        "eval: class/module creation did not yield a Class",
                    ));
                };
                let qualified = if owner == 0 {
                    name.clone()
                } else {
                    format!(
                        "{}::{name}",
                        crate::dispatch::class_name(crate::ClassId(owner)).unwrap_or_default()
                    )
                };
                crate::runtime_meta::name_runtime_class_if_anonymous(cid, &qualified);
                crate::constants::const_set(owner, &name, RubyValue::Class(cid));
                cid
            }
        };
        let class_val = RubyValue::Class(class_id);
        let mut body_env = Env {
            self_val: class_val,
            scope: fresh_scope(),
            box_id: env.box_id,
            definee: Definee::Class(class_id),
            block: None,
            method_args: None,
            src: Arc::clone(&env.src),
            cref: Some(class_id),
            cref_name: None,
            file: Arc::clone(&env.file),
            line: env.line,
        };
        eval_opt_stmts(body.and_then(|b| b.as_statements_node()), &mut body_env)
    }

    /// Resolve a `class`/`module` name node to `(owner class id, name)`. A
    /// bare `Foo` is owned by the current definee (the top level -> `Object`,
    /// id 0); a `A::B` path resolves `A` to a class/module value first.
    fn cpath_target(node: &Node<'_>, env: &mut Env) -> Result<(u32, String), Signal> {
        if let Some(cr) = node.as_constant_read_node() {
            let name = String::from_utf8_lossy(cr.name().as_slice()).into_owned();
            let owner = match &env.definee {
                Definee::Class(cid) => cid.0,
                Definee::Singleton(_) => 0,
            };
            return Ok((owner, name));
        }
        if let Some(cp) = node.as_constant_path_node() {
            let name = cp
                .name()
                .map(|n| String::from_utf8_lossy(n.as_slice()).into_owned())
                .ok_or_else(|| internal("eval: a computed `::` class name is not supported"))?;
            let owner = match cp.parent() {
                None => 0,
                Some(parent) => match eval_node(&parent, env)? {
                    RubyValue::Class(cid) => cid.0,
                    other => {
                        return Err(type_error!(
                            "{} is not a class/module",
                            other.inspect_string()
                        ));
                    }
                },
            };
            return Ok((owner, name));
        }
        Err(internal("eval: unsupported class/module name form"))
    }

    /// Build the `RProc` body for an eval-defined method. It captures the
    /// def's OWN source text (sliced from the enclosing eval source) and the
    /// box; on every invocation it re-parses that snippet, binds the call's
    /// args/block to the parameters, and interprets the body -- a `return`
    /// inside folds to the method's value here (the method boundary).
    fn make_eval_method(src: &Arc<str>, def: &DefNode<'_>, box_id: u32) -> Result<RProc, Signal> {
        let loc = def.location();
        let snippet: Arc<str> = Arc::from(&src[loc.start_offset()..loc.end_offset()]);
        // Methods behave like lambdas w.r.t. `return` (it exits the method),
        // which we implement directly by catching `Signal::Return` below.
        Ok(RProc::with_self_and_block(
            move |self_val: &RubyValue, args: &[RubyValue], block: Option<RubyValue>| {
                run_eval_method(&snippet, box_id, self_val, args, block)
            },
            RubyValue::Nil,
            -1,
            true,
        ))
    }

    /// One invocation of an eval-defined method: re-parse the stored snippet,
    /// bind params, interpret the body, and resolve `return`.
    fn run_eval_method(
        snippet: &str,
        box_id: u32,
        self_val: &RubyValue,
        args: &[RubyValue],
        block: Option<RubyValue>,
    ) -> Result<RubyValue, Signal> {
        let result = ruby_prism::parse(snippet.as_bytes());
        if let Some(err) = result.errors().next() {
            return Err(crate::dispatch::raise_error(
                "SyntaxError",
                err.message().to_string(),
            ));
        }
        let program = result
            .node()
            .as_program_node()
            .ok_or_else(|| internal("eval: re-parsed method body lost its ProgramNode"))?;
        let node = program.statements().body().iter().next();
        let def = node
            .as_ref()
            .and_then(Node::as_def_node)
            .ok_or_else(|| internal("eval: re-parsed method body is not a def"))?;

        let mut env = Env {
            self_val: self_val.clone(),
            scope: fresh_scope(),
            box_id,
            // A nested `def` inside a method body installs on the receiver's
            // class -- the common lexical case.
            definee: Definee::Class(self_val.class_id()),
            block,
            method_args: Some(args.to_vec()),
            src: Arc::from(snippet),
            // The body's constants resolve against the class the method was
            // defined into (`class_eval("def m = SOME_CONST")`), which is the
            // receiver's class at every invocation.
            cref: Some(self_val.class_id()),
            cref_name: None,
            file: Arc::from(EVAL_FILE),
            line: 1,
        };
        bind_params(&def, args, &env.block.clone(), &mut env)?;

        match eval_opt_stmts(def.body().and_then(|b| b.as_statements_node()), &mut env) {
            Err(Signal::Return(v)) => Ok(v),
            other => other,
        }
    }

    /// Bind a method call's positional args, keyword args, and block to the
    /// def's parameters, writing each into `env.scope`. Covers required,
    /// optional (with defaults), rest, post, required/optional keyword, and
    /// block parameters -- the shapes real methods use. Destructuring params
    /// and `**kwrest` are surfaced as a `NotImplementedError` for now.
    fn bind_params(
        def: &DefNode<'_>,
        args: &[RubyValue],
        block: &Option<RubyValue>,
        env: &mut Env,
    ) -> Result<(), Signal> {
        let Some(params) = def.parameters() else {
            if !args.is_empty() {
                return Err(arg_error!(
                    "wrong number of arguments (given {}, expected 0)",
                    args.len()
                ));
            }
            return Ok(());
        };

        // `def m(...)` -- prism models the forwarding parameter in the
        // keyword_rest slot. It binds NO names: a `f(...)` call site reads the
        // arguments back out of `env.method_args` and the block out of
        // `env.block`, so this signature only has to accept everything.
        let forwarding = params
            .keyword_rest()
            .is_some_and(|k| k.as_forwarding_parameter_node().is_some());

        let keywords: Vec<_> = params.keywords().iter().collect();
        let has_kw = !forwarding && (!keywords.is_empty() || params.keyword_rest().is_some());
        // With keyword params declared, a trailing Hash argument is the
        // keyword source (CRuby's implicit-hash-to-keywords rule).
        let (positional, kwargs): (&[RubyValue], Option<RubyValue>) = if has_kw {
            match args.last() {
                Some(RubyValue::Hash(_)) => {
                    (&args[..args.len() - 1], Some(args[args.len() - 1].clone()))
                }
                _ => (args, None),
            }
        } else {
            (args, None)
        };

        let reqs: Vec<_> = params.requireds().iter().collect();
        let opts: Vec<_> = params.optionals().iter().collect();
        let posts: Vec<_> = params.posts().iter().collect();
        let rest = params.rest();
        let has_rest = rest.is_some() || forwarding;
        let (n_req, n_opt, n_post) = (reqs.len(), opts.len(), posts.len());
        let min = n_req + n_post;

        // Arity check with CRuby's "given X, expected ..." shapes.
        let too_few = positional.len() < min;
        let too_many = !has_rest && positional.len() > min + n_opt;
        if too_few || too_many {
            let expected = if has_rest {
                format!("{min}+")
            } else if n_opt > 0 {
                format!("{min}..{}", min + n_opt)
            } else {
                format!("{min}")
            };
            return Err(arg_error!(
                "wrong number of arguments (given {}, expected {expected})",
                positional.len()
            ));
        }

        // Front-load requireds, back-load posts, the middle feeds optionals
        // then the rest splat.
        for (i, p) in reqs.iter().enumerate() {
            let name = required_name(p)?;
            env.scope.set(&name, positional[i].clone());
        }
        let mid_end = positional.len() - n_post;
        let mut cursor = n_req;
        for opt in &opts {
            let p = opt
                .as_optional_parameter_node()
                .ok_or_else(|| internal("eval: unsupported optional-parameter form"))?;
            let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
            let value = if cursor < mid_end {
                let v = positional[cursor].clone();
                cursor += 1;
                v
            } else {
                eval_node(&p.value(), env)?
            };
            env.scope.set(&name, value);
        }
        if let Some(r) = rest {
            // A named `*rest` collects the leftover middle; an anonymous `*`
            // has no name to bind, so it simply absorbs them.
            if let Some(rp) = r.as_rest_parameter_node()
                && let Some(name) = rp.name()
            {
                let collected: Vec<RubyValue> = positional[cursor..mid_end].to_vec();
                let name = String::from_utf8_lossy(name.as_slice()).into_owned();
                env.scope
                    .set(&name, RubyValue::Array(crate::array_new(collected)));
            }
        }
        for (i, p) in posts.iter().enumerate() {
            let name = required_name(p)?;
            env.scope.set(&name, positional[mid_end + i].clone());
        }

        if !forwarding {
            bind_keywords(&keywords, params.keyword_rest().is_some(), &kwargs, env)?;
        }

        if let Some(bp) = params.block()
            && let Some(name) = bp.name()
        {
            let name = String::from_utf8_lossy(name.as_slice()).into_owned();
            env.scope
                .set(&name, block.clone().unwrap_or(RubyValue::Nil));
        }
        Ok(())
    }

    /// Bind keyword parameters from the trailing kwargs hash. Required
    /// keywords missing → ArgumentError; unknown keywords with no `**rest`
    /// declared → ArgumentError -- both CRuby's messages.
    fn bind_keywords(
        keywords: &[Node<'_>],
        has_kw_rest: bool,
        kwargs: &Option<RubyValue>,
        env: &mut Env,
    ) -> Result<(), Signal> {
        // Symbol-keyed view of the passed keywords.
        let mut supplied: HashMap<String, RubyValue> = HashMap::new();
        if let Some(RubyValue::Hash(h)) = kwargs {
            for (k, v) in h.lock().values() {
                if let RubyValue::Symbol(s) = k {
                    supplied.insert(s.name().to_string(), v.clone());
                }
            }
        }
        let mut consumed: Vec<String> = Vec::new();
        for kw in keywords {
            if let Some(p) = kw.as_required_keyword_parameter_node() {
                let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
                let name = name.trim_end_matches(':').to_string();
                let value = supplied
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| arg_error!("missing keyword: :{name}"))?;
                consumed.push(name.clone());
                env.scope.set(&name, value);
            } else if let Some(p) = kw.as_optional_keyword_parameter_node() {
                let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
                let name = name.trim_end_matches(':').to_string();
                let value = match supplied.get(&name) {
                    Some(v) => v.clone(),
                    None => eval_node(&p.value(), env)?,
                };
                consumed.push(name.clone());
                env.scope.set(&name, value);
            } else {
                return Err(internal("eval: unsupported keyword-parameter form"));
            }
        }
        if !has_kw_rest {
            for key in supplied.keys() {
                if !consumed.contains(key) {
                    return Err(arg_error!("unknown keyword: :{key}"));
                }
            }
        }
        Ok(())
    }

    /// The name of a required (positional) parameter -- destructuring
    /// `(a, b)` params are not modeled yet.
    fn required_name(n: &Node<'_>) -> Result<String, Signal> {
        n.as_required_parameter_node()
            .map(|p| String::from_utf8_lossy(p.name().as_slice()).into_owned())
            .ok_or_else(|| internal("eval: destructuring method parameters are not supported yet"))
    }

    /// Evaluate an argument list into a flat value vector (splats flattened),
    /// pushing a trailing keyword hash for `k: v` arguments.
    fn collect_arguments(
        args: &NodeList<'_>,
        out: &mut Vec<RubyValue>,
        env: &mut Env,
    ) -> Result<(), Signal> {
        let mut kw_pairs: Vec<(RubyValue, RubyValue)> = Vec::new();
        for arg in args.iter() {
            if let Some(splat) = arg.as_splat_node() {
                if let Some(expr) = splat.expression() {
                    splat_into(out, eval_node(&expr, env)?);
                }
            } else if let Some(kh) = arg.as_keyword_hash_node() {
                for el in kh.elements().iter() {
                    let assoc = el.as_assoc_node().ok_or_else(|| {
                        internal(
                            "eval: `**` double-splat in keyword arguments is not supported yet",
                        )
                    })?;
                    kw_pairs.push((
                        eval_node(&assoc.key(), env)?,
                        eval_node(&assoc.value(), env)?,
                    ));
                }
            } else if arg.as_forwarding_arguments_node().is_some() {
                // `f(...)` inside a `def m(...)` -- forward every positional
                // argument the enclosing method received. Its block forwards
                // too, in `eval_call`, since the block isn't part of this list.
                out.extend(env.method_args.clone().unwrap_or_default());
            } else {
                out.push(eval_node(&arg, env)?);
            }
        }
        if !kw_pairs.is_empty() {
            out.push(RubyValue::Hash(crate::hash_new(kw_pairs)));
        }
        Ok(())
    }

    /// `defined?(operand)` -- the classification String, or `nil`. See the call
    /// site for which shapes are modelled and why.
    fn eval_defined(operand: &Node<'_>, env: &mut Env) -> Result<RubyValue, Signal> {
        let answer = |s: &str| Ok(RubyValue::Str(crate::string_new(s.to_string())));
        if let Some(call) = operand.as_call_node() {
            let receiver = match call.receiver() {
                Some(r) => eval_node(&r, env)?,
                None => env.self_val.clone(),
            };
            let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
            // Private methods count for an implicit receiver, as in Ruby.
            let all = call.receiver().is_none();
            let found =
                crate::dispatch::responds_to_or_missing(&receiver, Symbol::intern(&name), all)
                    .unwrap_or(false);
            return match found {
                true => answer("method"),
                false => Ok(RubyValue::Nil),
            };
        }
        if let Some(local) = operand.as_local_variable_read_node() {
            let name = String::from_utf8_lossy(local.name().as_slice()).into_owned();
            return match env.scope.defined(&name) {
                true => answer("local-variable"),
                false => Ok(RubyValue::Nil),
            };
        }
        if let Some(ivar) = operand.as_instance_variable_read_node() {
            let name = String::from_utf8_lossy(ivar.name().as_slice()).into_owned();
            let bare = name.trim_start_matches('@');
            return match crate::dispatch::ivar_defined(&env.self_val, bare) {
                true => answer("instance-variable"),
                false => Ok(RubyValue::Nil),
            };
        }
        if let Some(gvar) = operand.as_global_variable_read_node() {
            let name = String::from_utf8_lossy(gvar.name().as_slice()).into_owned();
            return match crate::globals::global_defined(env.box_id, &name) {
                true => answer("global-variable"),
                false => Ok(RubyValue::Nil),
            };
        }
        if operand.as_constant_read_node().is_some() || operand.as_constant_path_node().is_some() {
            // Resolution runs through the ordinary constant path, and a miss
            // raises -- which `defined?` turns into `nil` rather than
            // propagating, exactly as Ruby's never-raising contract requires.
            return match eval_node(operand, env) {
                Ok(_) => answer("constant"),
                Err(_) => Ok(RubyValue::Nil),
            };
        }
        if operand.as_self_node().is_some() {
            return answer("self");
        }
        if operand.as_nil_node().is_some() {
            return answer("expression");
        }
        answer("expression")
    }

    /// Whether `call`'s argument list is (or contains) the `...` forwarding
    /// form -- see `collect_arguments` and `eval_call`.
    fn forwards_arguments(call: &ruby_prism::CallNode<'_>) -> bool {
        call.arguments().is_some_and(|a| {
            a.arguments()
                .iter()
                .any(|arg| arg.as_forwarding_arguments_node().is_some())
        })
    }

    /// The value of a `return`/`break` with an argument list: a lone value as
    /// itself, several as an Array (`return 1, 2` -> `[1, 2]`).
    fn eval_arguments_single(
        args: &ruby_prism::ArgumentsNode<'_>,
        env: &mut Env,
    ) -> Result<RubyValue, Signal> {
        let mut values = Vec::new();
        collect_arguments(&args.arguments(), &mut values, env)?;
        Ok(match values.len() {
            1 => values.pop().unwrap(),
            _ => RubyValue::Array(crate::array_new(values)),
        })
    }

    fn eval_call(call: &ruby_prism::CallNode<'_>, env: &mut Env) -> Result<RubyValue, Signal> {
        let block = match call.block() {
            Some(b) => Some(eval_call_block(&b, env)?),
            // `f(...)` forwards the enclosing method's BLOCK as well as its
            // arguments -- the arguments themselves are picked up in
            // `collect_arguments`, which never sees the block slot.
            None if forwards_arguments(call) => env.block.clone(),
            None => None,
        };
        let receiver = match call.receiver() {
            Some(r) => eval_node(&r, env)?,
            None => env.self_val.clone(),
        };
        let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
        // A bare identifier that names a local of THIS scope is a variable
        // read, not a call. prism can't know that -- it parses the eval source
        // standalone, so `x` in `eval("x + 1", b)` arrives as a vcall while
        // `x = 1` in the same source arrives as a real write. CRuby seeds its
        // parser with the binding's locals; the equivalent here is to check
        // the scope before dispatching.
        if call.receiver().is_none()
            && call.arguments().is_none()
            && call.block().is_none()
            && let Some(v) = env.scope.get(&name)
        {
            return Ok(v);
        }
        // `binding` inside an eval names THIS scope -- the very cells the
        // source has been assigning, which is what makes ERB's
        // `b.eval("tap {|;v| break binding}")` hand back a usable scope.
        if name == "binding" && call.receiver().is_none() && call.arguments().is_none() {
            return Ok(crate::builtins::binding::binding_value(
                env.self_val.clone(),
                Arc::clone(&env.scope),
                env.file.to_string(),
                env.line,
                env.box_id,
                env.cref,
            ));
        }
        let sym = crate::Symbol::intern(&name);

        let mut args = Vec::new();
        if let Some(arg_node) = call.arguments() {
            collect_arguments(&arg_node.arguments(), &mut args, env)?;
        }
        crate::dispatch::send_value_in(env.box_id, &receiver, sym, &args, block)
    }

    /// Build the `Proc` for a literal block (`{ ... }` / `do ... end`) passed
    /// to a call inside eval. Like an eval-defined method it re-parses its own
    /// source per invocation; unlike one it is NOT a lambda (a `return` inside
    /// is a non-local return, left to propagate). A `&proc_arg` block-pass is
    /// handled separately (it's already a Proc value).
    fn eval_call_block(node: &Node<'_>, env: &mut Env) -> Result<RubyValue, Signal> {
        if let Some(bp) = node.as_block_argument_node() {
            // `foo(&blk)` -- forward an existing Proc/block value.
            return match bp.expression() {
                Some(e) => eval_node(&e, env),
                None => Ok(RubyValue::Nil),
            };
        }
        let block = node
            .as_block_node()
            .ok_or_else(|| internal("eval: unsupported block form on a call inside eval"))?;
        let loc = block.location();
        let snippet: Arc<str> = Arc::from(&env.src[loc.start_offset()..loc.end_offset()]);
        let self_val = env.self_val.clone();
        let box_id = env.box_id;
        let proc = RProc::with_self_and_block(
            move |bound_self: &RubyValue, args: &[RubyValue], _blk: Option<RubyValue>| {
                run_eval_block(&snippet, box_id, bound_self, args)
            },
            self_val,
            -1,
            false,
        );
        Ok(RubyValue::Proc(proc))
    }

    /// Where a `def` inside a re-parsed block installs: the CLASS itself when
    /// `self` is one (`Foo.module_eval { def m; end }` defines `Foo#m` -- the
    /// default definee of a module_eval body), else the receiver's class.
    ///
    /// Without the first case the definee was `Class`, and the method landed
    /// somewhere nothing could call it -- which is what forwardable's
    /// delegators hit once they started parsing.
    fn block_definee(self_val: &RubyValue) -> crate::ClassId {
        match self_val {
            RubyValue::Class(cid) => *cid,
            other => other.class_id(),
        }
    }

    /// The receiver-less call a stored block snippet is re-parsed under -- see
    /// `run_eval_block`. Never actually dispatched; only its block is read.
    const BLOCK_WRAP_CALL: &str = "__zeo_eval_block ";

    /// One invocation of an eval-defined block: re-parse the `{...}`/`do...end`
    /// snippet, bind its params to the yielded args, interpret the body.
    fn run_eval_block(
        snippet: &str,
        box_id: u32,
        self_val: &RubyValue,
        args: &[RubyValue],
    ) -> Result<RubyValue, Signal> {
        // prism only parses a block in CALL position, so the stored
        // `{...}`/`do...end` text is re-parsed as the block of a synthetic
        // no-op call. Parsing it bare made a brace block read as a Hash and a
        // `do...end` one fail outright ("unexpected 'do'"), which is how
        // forwardable's `eval("proc do ... end")` delegators died.
        //
        // The WRAPPED text becomes `env.src`, not the bare snippet: a nested
        // block slices its own source out of that same buffer by location, so
        // the two must be the same string.
        let wrapped = format!("{BLOCK_WRAP_CALL}{snippet}");
        let snippet = wrapped.as_str();
        let result = ruby_prism::parse(snippet.as_bytes());
        if let Some(err) = result.errors().next() {
            return Err(crate::dispatch::raise_error(
                "SyntaxError",
                err.message().to_string(),
            ));
        }
        let program = result
            .node()
            .as_program_node()
            .ok_or_else(|| internal("eval: re-parsed block lost its ProgramNode"))?;
        // The snippet is a lone call whose single argument is the block; dig it
        // back out. Simpler: prism parses `{ |x| x }` bare as a BlockNode only
        // in call context, so we wrap-parse it as the block of a no-op call.
        let block = program
            .statements()
            .body()
            .iter()
            .next()
            .as_ref()
            .and_then(Node::as_call_node)
            .and_then(|c| c.block())
            .and_then(|b| b.as_block_node());
        let Some(block) = block else {
            return Err(internal("eval: re-parsed block body is not a block"));
        };
        let mut env = Env {
            self_val: self_val.clone(),
            scope: fresh_scope(),
            box_id,
            definee: if crate::runtime_meta::singleton_definee(self_val) {
                Definee::Singleton(self_val.clone())
            } else {
                Definee::Class(block_definee(self_val))
            },
            block: None,
            method_args: None,
            src: Arc::from(snippet),
            cref: None,
            cref_name: None,
            file: Arc::from(EVAL_FILE),
            line: 1,
        };
        bind_block_params(&block, args, &mut env)?;
        eval_opt_stmts(block.body().and_then(|b| b.as_statements_node()), &mut env)
    }

    /// Bind a block's parameters to the yielded args -- lenient like `yield`
    /// (extra args dropped, missing ones nil), with `*rest` and auto-splat of
    /// a lone Array across multiple params.
    fn bind_block_params(
        block: &ruby_prism::BlockNode<'_>,
        args: &[RubyValue],
        env: &mut Env,
    ) -> Result<(), Signal> {
        let Some(params) = block
            .parameters()
            .and_then(|p| p.as_block_parameters_node())
        else {
            return Ok(());
        };
        let Some(params) = params.parameters() else {
            return Ok(());
        };
        let reqs: Vec<_> = params.requireds().iter().collect();
        // Auto-splat: `[[1,2]].each { |a, b| }` spreads the lone Array when the
        // block declares more than one parameter (Ruby's block-arg rule).
        let effective: Vec<RubyValue> = if reqs.len() > 1 && args.len() == 1 {
            match &args[0] {
                RubyValue::Array(a) => a.lock().to_vec(),
                other => vec![other.clone()],
            }
        } else {
            args.to_vec()
        };
        for (i, p) in reqs.iter().enumerate() {
            let name = required_name(p)?;
            env.scope
                .set(&name, effective.get(i).cloned().unwrap_or(RubyValue::Nil));
        }
        if let Some(rest) = params.rest()
            && let Some(rp) = rest.as_rest_parameter_node()
            && let Some(name) = rp.name()
        {
            let collected: Vec<RubyValue> = effective.iter().skip(reqs.len()).cloned().collect();
            let name = String::from_utf8_lossy(name.as_slice()).into_owned();
            env.scope
                .set(&name, RubyValue::Array(crate::array_new(collected)));
        }
        Ok(())
    }

    /// Concatenate a (possibly interpolated) string literal's parts. A literal
    /// part contributes its bytes; a `#{...}` part evaluates and is coerced
    /// with `to_s`.
    fn eval_interpolated(parts: &NodeList<'_>, env: &mut Env) -> Result<RubyValue, Signal> {
        let mut out = String::new();
        for part in parts.iter() {
            if let Some(s) = part.as_string_node() {
                out.push_str(&String::from_utf8_lossy(s.unescaped()));
            } else if let Some(embedded) = part.as_embedded_statements_node() {
                let value = eval_opt_stmts(embedded.statements(), env)?;
                out.push_str(&stringify(&value, env.box_id)?);
            } else if let Some(embedded) = part.as_embedded_variable_node() {
                let value = eval_node(&embedded.variable(), env)?;
                out.push_str(&stringify(&value, env.box_id)?);
            } else {
                return Err(internal("eval: unsupported string interpolation part"));
            }
        }
        Ok(RubyValue::Str(crate::string_new(out)))
    }

    /// `to_s` a value into a Rust `String` for interpolation.
    fn stringify(v: &RubyValue, box_id: u32) -> Result<String, Signal> {
        if let RubyValue::Str(s) = v {
            return Ok(s.lock().to_utf8_lossy().into_owned());
        }
        let s =
            crate::dispatch::send_value_in(box_id, v, crate::Symbol::intern("to_s"), &[], None)?;
        match s {
            RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
            other => Ok(other.inspect_string()),
        }
    }

    /// Flatten a splat operand into an argument/element vector: an `Array`
    /// contributes its elements, anything else contributes itself (a crude
    /// stand-in for `to_a` coercion -- broadened in a later increment).
    fn splat_into(out: &mut Vec<RubyValue>, v: RubyValue) {
        match v {
            RubyValue::Array(a) => out.extend(a.lock().iter().cloned()),
            other => out.push(other),
        }
    }

    /// Look a constant up in `owner`'s table, raising CRuby's `NameError` when
    /// it is not defined. Falls back (top level only) to the class registry so
    /// a bare class/module NAME (`Integer`, `Math`, a user `Widget`) resolves
    /// even though codegen never `const_set`s those -- it bakes them in at
    /// compile time, which the VM can't.
    fn const_lookup(owner: u32, name: &str) -> Result<RubyValue, Signal> {
        if let Some(v) = crate::constants::const_get(owner, name) {
            return Ok(v);
        }
        top_level_class(owner, name)
    }

    /// The `::` operator's lookup: `owner`'s ancestry, minus what `Object`
    /// itself owns -- so `K::Errno` raises rather than answering the top-level
    /// `Errno`, exactly as the compiled path does.
    fn const_lookup_scoped(owner: u32, name: &str) -> Result<RubyValue, Signal> {
        if let Some(v) = crate::constants::const_get_scoped(owner, name) {
            return Ok(v);
        }
        // A nested class/module is registered by its qualified name rather than
        // written to the constants table, so it needs its own probe.
        if owner != 0 {
            let path = crate::dispatch::class_name(crate::ClassId(owner))
                .map(|scope| format!("{scope}::{name}"));
            if let Some(cid) = path.and_then(|p| crate::dispatch::class_id_by_name(&p)) {
                return Ok(RubyValue::Class(cid));
            }
        }
        top_level_class(owner, name)
    }

    /// The class registry is `Object`'s half of the constant table: codegen
    /// bakes a bare class NAME in at compile time, so nothing ever `const_set`s
    /// one and only the top level can answer for it.
    fn top_level_class(owner: u32, name: &str) -> Result<RubyValue, Signal> {
        if owner == 0
            && let Some(cid) = crate::dispatch::class_id_by_name(name)
        {
            return Ok(RubyValue::Class(cid));
        }
        Err(name_error!("uninitialized constant {name}"))
    }

    fn truthy(v: &RubyValue) -> bool {
        !matches!(v, RubyValue::Nil | RubyValue::Bool(false))
    }

    /// prism reports an instance-variable name WITH its `@` sigil (`@x`); the
    /// runtime stores it without (`x`).
    fn ivar_key(raw: &[u8]) -> String {
        let name = String::from_utf8_lossy(raw);
        name.strip_prefix('@').unwrap_or(&name).to_string()
    }

    /// An eval-VM limitation surfaced as a catchable `NotImplementedError`
    /// (our gap, not a Ruby error) rather than a panic.
    fn internal(msg: &str) -> Signal {
        crate::dispatch::raise_error("NotImplementedError", msg.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // These exercise the interpreter's STRUCTURAL evaluation only -- no
        // method dispatch -- so they need no installed class registry (which
        // `dispatch::send` requires). Arithmetic and real method calls are
        // covered end-to-end by the `--features eval-vm` e2e suite.
        fn eval(src: &str) -> RubyValue {
            super::eval_string(src, RubyValue::Nil, 0, super::EvalMode::Caller).expect("eval ok")
        }

        #[test]
        fn integer_and_float_literals() {
            assert!(matches!(eval("42"), RubyValue::Int(42)));
            assert!(matches!(eval("3.5"), RubyValue::Float(f) if f == 3.5));
        }

        #[test]
        fn empty_program_is_nil() {
            assert!(matches!(eval(""), RubyValue::Nil));
        }

        #[test]
        fn local_assignment_then_read() {
            assert!(matches!(eval("x = 7; x"), RubyValue::Int(7)));
        }

        #[test]
        fn unassigned_declared_local_is_nil() {
            // `y` is assigned only in the never-taken branch, so prism parses
            // the trailing `y` as a local read; it must answer nil, not error.
            assert!(matches!(eval("y = 1 if false; y"), RubyValue::Nil));
        }

        #[test]
        fn if_selects_the_true_branch() {
            assert!(matches!(
                eval("if true then 1 else 2 end"),
                RubyValue::Int(1)
            ));
            assert!(matches!(
                eval("if false then 1 else 2 end"),
                RubyValue::Int(2)
            ));
            assert!(matches!(eval("if false then 1 end"), RubyValue::Nil));
        }

        #[test]
        fn unless_inverts() {
            assert!(matches!(
                eval("unless false then 1 else 2 end"),
                RubyValue::Int(1)
            ));
        }

        #[test]
        fn elsif_chain() {
            assert!(matches!(
                eval("if false then 1 elsif true then 2 else 3 end"),
                RubyValue::Int(2)
            ));
        }

        #[test]
        fn and_or_short_circuit_return_the_operand() {
            assert!(matches!(eval("nil && 1"), RubyValue::Nil));
            assert!(matches!(eval("false || 5"), RubyValue::Int(5)));
            assert!(matches!(eval("3 && 4"), RubyValue::Int(4)));
            assert!(matches!(eval("7 || 8"), RubyValue::Int(7)));
        }

        #[test]
        fn while_loop_runs_and_returns_nil() {
            // Counts up entirely in locals -- no dispatch.
            let v = super::eval_string(
                "i = 0; i = 3 while false; i",
                RubyValue::Nil,
                0,
                super::EvalMode::Caller,
            )
            .expect("eval ok");
            assert!(matches!(v, RubyValue::Int(0)));
        }

        #[test]
        fn array_literal() {
            match eval("[1, 2, 3]") {
                RubyValue::Array(a) => {
                    let g = a.lock();
                    assert_eq!(g.len(), 3);
                    assert!(matches!(g[0], RubyValue::Int(1)));
                }
                other => panic!("expected array, got {other:?}"),
            }
        }

        #[test]
        fn range_literal_endpoints() {
            match eval("1..5") {
                RubyValue::Range(Some(lo), Some(hi), excl) => {
                    assert!(matches!(*lo, RubyValue::Int(1)));
                    assert!(matches!(*hi, RubyValue::Int(5)));
                    assert!(!excl);
                }
                other => panic!("expected range, got {other:?}"),
            }
        }

        // A parse failure routes through `raise_error("SyntaxError", ..)`.
        // Registry-less (as here), `raise_error` panics while constructing the
        // exception object -- so the panic message IS the proof the SyntaxError
        // path was taken, mirroring the `runtime_meta` tests' convention. In a
        // real program (registry installed) this is an ordinary catchable raise.
        #[test]
        #[should_panic(expected = "SyntaxError")]
        fn parse_error_is_a_syntax_error() {
            let _ = super::eval_string("def", RubyValue::Nil, 0, super::EvalMode::Caller);
        }
    }
}
