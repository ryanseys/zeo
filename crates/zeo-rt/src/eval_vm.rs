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
//! invocation. What it does NOT yet see is the CALLER's own local variables:
//! those live as Rust stack slots the interpreter can't reach without codegen
//! materializing a `Binding` (the next increment, with a first-class
//! `binding`). A local ASSIGNED inside an eval is visible to later statements
//! of the SAME eval, held in `Env::locals`.

// Feature-split imports: the interpreter (`mod imp`, eval-vm on) raises
// NameError for unresolved constants; the feature-off stub raises
// NotImplementedError. Each import exists only where its arm compiles, or
// the other build flags it unused.
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

#[cfg(feature = "eval-vm")]
mod imp {
    use super::*;
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
        /// Locals defined DURING this eval (not the caller's). A local read
        /// prism resolved (it was assigned earlier in the eval source) but
        /// which no executed statement has written yet reads as `nil`, exactly
        /// as a declared-but-unassigned Ruby local does.
        locals: HashMap<String, RubyValue>,
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
    }

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
        let definee = initial_definee(&self_val, mode);
        let mut env = Env {
            self_val,
            locals: HashMap::new(),
            box_id,
            definee,
            block: None,
            method_args: None,
            src: Arc::from(src),
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
                        return Err(type_error!(
                            "{} is not a class/module",
                            other.inspect_string()
                        ));
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
            locals: HashMap::new(),
            box_id: env.box_id,
            definee: Definee::Class(class_id),
            block: None,
            method_args: None,
            src: Arc::clone(&env.src),
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
            locals: HashMap::new(),
            box_id,
            // A nested `def` inside a method body installs on the receiver's
            // class -- the common lexical case.
            definee: Definee::Class(self_val.class_id()),
            block,
            method_args: Some(args.to_vec()),
            src: Arc::from(snippet),
        };
        bind_params(&def, args, &env.block.clone(), &mut env)?;

        match eval_opt_stmts(def.body().and_then(|b| b.as_statements_node()), &mut env) {
            Err(Signal::Return(v)) => Ok(v),
            other => other,
        }
    }

    /// Bind a method call's positional args, keyword args, and block to the
    /// def's parameters, writing each into `env.locals`. Covers required,
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

        let keywords: Vec<_> = params.keywords().iter().collect();
        let has_kw = !keywords.is_empty() || params.keyword_rest().is_some();
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
        let has_rest = rest.is_some();
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
            env.locals.insert(name, positional[i].clone());
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
            env.locals.insert(name, value);
        }
        if let Some(r) = rest {
            // A named `*rest` collects the leftover middle; an anonymous `*`
            // has no name to bind, so it simply absorbs them.
            if let Some(rp) = r.as_rest_parameter_node() {
                if let Some(name) = rp.name() {
                    let collected: Vec<RubyValue> = positional[cursor..mid_end].to_vec();
                    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
                    env.locals
                        .insert(name, RubyValue::Array(crate::array_new(collected)));
                }
            }
        }
        for (i, p) in posts.iter().enumerate() {
            let name = required_name(p)?;
            env.locals.insert(name, positional[mid_end + i].clone());
        }

        bind_keywords(&keywords, params.keyword_rest().is_some(), &kwargs, env)?;

        if let Some(bp) = params.block() {
            if let Some(name) = bp.name() {
                let name = String::from_utf8_lossy(name.as_slice()).into_owned();
                env.locals
                    .insert(name, block.clone().unwrap_or(RubyValue::Nil));
            }
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
                env.locals.insert(name, value);
            } else if let Some(p) = kw.as_optional_keyword_parameter_node() {
                let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
                let name = name.trim_end_matches(':').to_string();
                let value = match supplied.get(&name) {
                    Some(v) => v.clone(),
                    None => eval_node(&p.value(), env)?,
                };
                consumed.push(name.clone());
                env.locals.insert(name, value);
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
            } else {
                out.push(eval_node(&arg, env)?);
            }
        }
        if !kw_pairs.is_empty() {
            out.push(RubyValue::Hash(crate::hash_new(kw_pairs)));
        }
        Ok(())
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
            None => None,
        };
        let receiver = match call.receiver() {
            Some(r) => eval_node(&r, env)?,
            None => env.self_val.clone(),
        };
        let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
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

    /// One invocation of an eval-defined block: re-parse the `{...}`/`do...end`
    /// snippet, bind its params to the yielded args, interpret the body.
    fn run_eval_block(
        snippet: &str,
        box_id: u32,
        self_val: &RubyValue,
        args: &[RubyValue],
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
            locals: HashMap::new(),
            box_id,
            definee: Definee::Class(self_val.class_id()),
            block: None,
            method_args: None,
            src: Arc::from(snippet),
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
                RubyValue::Array(a) => a.lock().clone(),
                other => vec![other.clone()],
            }
        } else {
            args.to_vec()
        };
        for (i, p) in reqs.iter().enumerate() {
            let name = required_name(p)?;
            env.locals
                .insert(name, effective.get(i).cloned().unwrap_or(RubyValue::Nil));
        }
        if let Some(rest) = params.rest() {
            if let Some(rp) = rest.as_rest_parameter_node() {
                if let Some(name) = rp.name() {
                    let collected: Vec<RubyValue> =
                        effective.iter().skip(reqs.len()).cloned().collect();
                    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
                    env.locals
                        .insert(name, RubyValue::Array(crate::array_new(collected)));
                }
            }
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
        if owner == 0 {
            if let Some(cid) = crate::dispatch::class_id_by_name(name) {
                return Ok(RubyValue::Class(cid));
            }
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
