//! Runtime string `eval` (#97 stage 2) -- a tree-walking interpreter over
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
//! **Scope of this stage: top-level-first.** An `eval` runs with a correct
//! `self` (its receiver's ivars, implicit-self calls, constants, globals)
//! but does NOT see the CALLER's own local variables -- those live as Rust
//! stack slots the interpreter can't reach without codegen materializing a
//! `Binding`. That materialization (and a first-class `binding`) is the next
//! increment; a local ASSIGNED inside an eval is visible to later statements
//! of the SAME eval, held in `Env::locals`.

use crate::{RubyValue, Signal};

/// Evaluate `src` as a standalone chunk of Ruby with `self` bound to
/// `self_val`, resolving constants/globals against `box_id`. A parse failure
/// becomes a catchable `SyntaxError`; the value of the last statement is
/// returned (`nil` for an empty program).
///
/// Compiled without the `eval-vm` feature this is the honest stub: a
/// `NotImplementedError` naming the missing feature, mirroring the runtime's
/// other "not compiled in" surfaces.
pub fn eval_string(src: &str, self_val: RubyValue, box_id: u32) -> Result<RubyValue, Signal> {
    #[cfg(feature = "eval-vm")]
    {
        imp::eval_string(src, self_val, box_id)
    }
    #[cfg(not(feature = "eval-vm"))]
    {
        let _ = (src, self_val, box_id);
        Err(crate::dispatch::raise_error(
            "NotImplementedError",
            "string eval requires the eval VM (build spinel-rt with --features eval-vm)"
                .to_string(),
        ))
    }
}

/// The dynamic-`eval` entry every dispatch site funnels through: coerce the
/// source argument to a String (Ruby raises `TypeError` for anything else,
/// even a Symbol) and evaluate it with `self` bound to `self_val`. Keeping the
/// coercion here means every caller -- `Kernel#eval`, `instance_eval`,
/// `class_eval` -- shares one definition of "what counts as evalable source".
pub fn eval_value(
    src: RubyValue,
    self_val: RubyValue,
    box_id: u32,
) -> Result<RubyValue, Signal> {
    match &src {
        RubyValue::Str(s) => {
            let code = s.lock().to_utf8_lossy().into_owned();
            eval_string(&code, self_val, box_id)
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into String",
                crate::builtins::convert_name_of(other)
            ),
        )),
    }
}

#[cfg(feature = "eval-vm")]
mod imp {
    use super::*;
    use ruby_prism::{Node, NodeList, StatementsNode};
    use std::collections::HashMap;

    /// The lexical environment a single `eval` runs in -- see the module docs
    /// on the "top-level-first" scope decision.
    struct Env {
        /// The `self` every implicit-receiver call and `@ivar` access binds to.
        self_val: RubyValue,
        /// Locals defined DURING this eval (not the caller's). A local read
        /// prism resolved (it was assigned earlier in the eval source) but
        /// which no executed statement has written yet reads as `nil`, exactly
        /// as a declared-but-unassigned Ruby local does.
        locals: HashMap<String, RubyValue>,
        /// Defining box for constant/global resolution (Phase 18).
        box_id: u32,
    }

    pub(super) fn eval_string(
        src: &str,
        self_val: RubyValue,
        box_id: u32,
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
            self_val,
            locals: HashMap::new(),
            box_id,
        };
        eval_list(&program.statements().body(), &mut env)
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
        if let Some(istr) = node.as_interpolated_string_node() {
            return eval_interpolated(&istr.parts(), env);
        }

        // ---- locals ---------------------------------------------------------
        if let Some(lvr) = node.as_local_variable_read_node() {
            let name = String::from_utf8_lossy(lvr.name().as_slice()).into_owned();
            return Ok(env.locals.get(&name).cloned().unwrap_or(RubyValue::Nil));
        }
        if let Some(lvw) = node.as_local_variable_write_node() {
            let name = String::from_utf8_lossy(lvw.name().as_slice()).into_owned();
            let value = eval_node(&lvw.value(), env)?;
            env.locals.insert(name, value.clone());
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
            // Top-level-first scope: constants resolve against the root
            // (`Object`, class id 0), the lexical top of an eval'd chunk.
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
                        return Err(crate::dispatch::raise_error(
                            "TypeError",
                            format!("{} is not a class/module", other.inspect_string()),
                        ))
                    }
                },
            };
            return const_lookup(owner, &name);
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

        // ---- method calls ---------------------------------------------------
        if let Some(call) = node.as_call_node() {
            return eval_call(&call, env);
        }

        Err(internal(
            "eval: unsupported syntax in this build (the eval VM does not yet cover this node)",
        ))
    }

    fn eval_call(call: &ruby_prism::CallNode<'_>, env: &mut Env) -> Result<RubyValue, Signal> {
        if call.block().is_some() {
            return Err(internal(
                "eval: a block passed to a call inside `eval` is not supported yet",
            ));
        }
        let receiver = match call.receiver() {
            Some(r) => eval_node(&r, env)?,
            None => env.self_val.clone(),
        };
        let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
        let sym = crate::Symbol::intern(&name);

        let mut args = Vec::new();
        if let Some(arg_node) = call.arguments() {
            for arg in arg_node.arguments().iter() {
                if let Some(splat) = arg.as_splat_node() {
                    if let Some(expr) = splat.expression() {
                        splat_into(&mut args, eval_node(&expr, env)?);
                    }
                } else if arg.as_keyword_hash_node().is_some() {
                    return Err(internal(
                        "eval: keyword arguments in a call inside `eval` are not supported yet",
                    ));
                } else {
                    args.push(eval_node(&arg, env)?);
                }
            }
        }
        crate::dispatch::send_value_in(env.box_id, &receiver, sym, &args, None)
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
        let s = crate::dispatch::send_value_in(box_id, v, crate::Symbol::intern("to_s"), &[], None)?;
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
        if owner == 0 {
            if let Some(cid) = crate::dispatch::class_id_by_name(name) {
                return Ok(RubyValue::Class(cid));
            }
        }
        Err(crate::dispatch::raise_error(
            "NameError",
            format!("uninitialized constant {name}"),
        ))
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
            super::eval_string(src, RubyValue::Nil, 0).expect("eval ok")
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
            assert!(matches!(eval("if true then 1 else 2 end"), RubyValue::Int(1)));
            assert!(matches!(eval("if false then 1 else 2 end"), RubyValue::Int(2)));
            assert!(matches!(eval("if false then 1 end"), RubyValue::Nil));
        }

        #[test]
        fn unless_inverts() {
            assert!(matches!(eval("unless false then 1 else 2 end"), RubyValue::Int(1)));
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
            let v = super::eval_string("i = 0; i = 3 while false; i", RubyValue::Nil, 0)
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
            let _ = super::eval_string("def", RubyValue::Nil, 0);
        }
    }
}
