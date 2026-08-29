//! Call lowering: the ordinary `CallNode` arm (`lower_call_node` and its
//! named-arm recognizers), `CallNode` argument/kwarg lowering helpers, block
//! lowering (`lower_block`, block parameters), and the call-argument
//! splat/forwarding machinery.

use super::consts::{box_rooted_path, constant_path_name};
use super::defs::{self, const_is_assigned, lower_params};
use super::eval_splice::{lower_box_eval, single_literal_string_arg};
use super::{
    PResult, assign, autoload_feature, computed_relative_demand_dir, context, current_dir_str,
    features, lower_array_elem, lower_body, lower_kwargs, lower_node,
};
use crate::hir::{ArrayElem, Hir, HirNode, KwArg, NodeId, Params, RaiseCause, StrPart, Visibility};
use ruby_prism::{CallNode, Node, ParseResult};

/// Shared by `lower_block` and lambda lowering (`-> (x) { }`/`lambda { }`):
/// both a `BlockNode` and a `LambdaNode` expose their own `.parameters()` as
/// the identical `Option<Node>` shape (a `BlockParametersNode`, or the
/// `_1`/`it` sugar nodes -- confirmed via `Prism.parse` directly, not just
/// inferred from the bindings).
pub(crate) fn lower_block_like_params(
    result: &ParseResult,
    hir: &mut Hir,
    params: Option<Node<'_>>,
) -> PResult<Params> {
    match params {
        None => Ok(Params::default()),
        // `_1`/`_2`/... -- `NumberedParametersNode { maximum }` reports the
        // highest `_N` referenced in the body; synthesize that many plain
        // required params (pure lowering-time sugar, no new HIR).
        Some(p) if p.as_numbered_parameters_node().is_some() => {
            let n = p.as_numbered_parameters_node().unwrap().maximum();
            Ok(Params {
                required: (1..=n).map(|i| format!("_{i}")).collect(),
                ..Params::default()
            })
        }
        // `it` -- `ItParametersNode` carries no fields (the body just
        // references bare `it`); synthesize a single required param.
        Some(p) if p.as_it_parameters_node().is_some() => Ok(Params {
            required: vec!["it".to_string()],
            ..Params::default()
        }),
        Some(p) => {
            let bp = p
                .as_block_parameters_node()
                .ok_or("unsupported block parameter form (zeo limitation)")?;
            let mut params = lower_params(result, hir, bp.parameters())?;
            // `|x; sum|`'s block-locals -- prism keeps them on the
            // `BlockParametersNode` itself (`locals()`), not in the
            // `ParametersNode` `lower_params` handles, precisely because
            // they are not parameters. See `Params::block_locals`' docs.
            params.block_locals = bp
                .locals()
                .iter()
                .filter_map(|l| l.as_block_local_variable_node())
                .map(|l| String::from_utf8_lossy(l.name().as_slice()).into_owned())
                .collect();
            Ok(params)
        }
    }
}

pub(crate) fn lower_block(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<NodeId> {
    let block = node
        .as_block_node()
        .ok_or("expected a block (`{ }` or `do..end`)")?;
    let mut params = lower_block_like_params(result, hir, block.parameters())?;
    // Prism's block-scope local table minus the names this block binds as
    // parameters (and explicit `;`-block-locals) yields exactly the IMPLICIT
    // block-locals -- names first-assigned inside the body, which Ruby resets
    // to nil per invocation. See `Params::implicit_block_locals`.
    let bound: std::collections::HashSet<String> = params.bound_names().into_iter().collect();
    params.implicit_block_locals = block
        .locals()
        .iter()
        .map(|c| String::from_utf8_lossy(c.as_slice()).into_owned())
        .filter(|n| !bound.contains(n))
        .collect();
    let body = lower_body(result, hir, block.body())?;
    Ok(hir.push(HirNode::Block {
        params: Box::new(params),
        body,
    }))
}

/// Splits a call's raw argument list into (positional `ArrayElem`s, an
/// ordered `KwArg` list, an optional forwarded block). A trailing
/// `KeywordHashNode` (`foo(x: 1, **h)`) is the only prism shape recognized as
/// keyword arguments (lowered via `lower_kwargs`); every other entry lowers as
/// a positional argument via `lower_array_elem`. The returned `Option<NodeId>`
/// is a `...`-forwarded block (`&__fwd_blk`); a call's own literal block lives
/// outside this function.
pub(crate) fn lower_call_args(
    result: &ParseResult,
    hir: &mut Hir,
    arguments: Option<ruby_prism::ArgumentsNode<'_>>,
) -> PResult<(Vec<ArrayElem>, Vec<KwArg>, Option<NodeId>)> {
    let Some(arguments) = arguments else {
        return Ok((Vec::new(), Vec::new(), None));
    };
    let mut list: Vec<_> = arguments.arguments().iter().collect();
    // `n(...)` inside `def m(...)` -- a `ForwardingArgumentsNode` in the
    // list. Expands to the three internal params `lower_params`
    // synthesized: `*__fwd_rest, **__fwd_kw, &__fwd_blk` (the block half
    // returned separately -- a call's block slot lives outside this
    // function). `**__fwd_kw` enters the ordered `kwargs` list as a trailing
    // double-splat.
    if let Some(pos) = list
        .iter()
        .position(|n| n.as_forwarding_arguments_node().is_some())
    {
        list.remove(pos);
        let fwd_kw = hir.push(HirNode::LocalRead("__fwd_kw".to_string()));
        let fwd_block = Some(hir.push(HirNode::LocalRead("__fwd_blk".to_string())));
        // The rest-splat slots in positionally where `...` was written.
        let rest_read = hir.push(HirNode::LocalRead("__fwd_rest".to_string()));
        let mut args = Vec::new();
        for (i, n) in list.iter().enumerate() {
            if i == pos {
                args.push(ArrayElem::Splat(rest_read));
            }
            args.push(lower_array_elem_or_anon(result, hir, n)?);
        }
        if pos >= list.len() {
            args.push(ArrayElem::Splat(rest_read));
        }
        return Ok((args, vec![KwArg::DoubleSplat(fwd_kw)], fwd_block));
    }
    let kwargs = match list.last().and_then(|n| n.as_keyword_hash_node()) {
        Some(kw) => {
            list.pop();
            let elements: Vec<Node<'_>> = kw.elements().iter().collect();
            lower_kwargs(result, hir, &elements)?
        }
        None => Vec::new(),
    };
    let args = list
        .iter()
        .map(|n| lower_array_elem_or_anon(result, hir, n))
        .collect::<PResult<Vec<_>>>()?;
    Ok((args, kwargs, None))
}

/// `lower_array_elem`, plus the CALL-argument-only anonymous `*` forwarding
/// form (`n(*)` inside `def m(*)`) -- an array literal's own bare `*` stays
/// rejected in `lower_array_elem` itself.
fn lower_array_elem_or_anon(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<ArrayElem> {
    if let Some(splat) = node.as_splat_node()
        && splat.expression().is_none()
    {
        return Ok(ArrayElem::Splat(
            hir.push(HirNode::LocalRead("__anon_rest".to_string())),
        ));
    }
    lower_array_elem(result, hir, node)
}

/// The call-adjacent family of [`super::lower_node_inner`]'s recognizer
/// chain: lambda literals, `yield`, and both `super` spellings. The ordinary
/// `CallNode` arm lives below (`lower_call_node`), dispatched separately by
/// `lower_node_inner`. `Ok(None)` = not this family's node.
pub(crate) fn try_lower(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<Option<NodeId>> {
    // `-> (x) { ... }` -- a real `ruby-prism` node (unlike `lambda { }`
    // below, which is an ordinary method call). See `hir::HirNode::Lambda`'s
    // docs.
    if let Some(lambda) = node.as_lambda_node() {
        let params = lower_block_like_params(result, hir, lambda.parameters())?;
        let body = lower_body(result, hir, lambda.body())?;
        return Ok(Some(hir.push(HirNode::Lambda {
            params: Box::new(params),
            body,
            method_body: false,
        })));
    }

    // `yield` / `yield(args)` -- a real, distinct `ruby-prism` node (not an
    // ordinary call), unlike `block_given?` below. Reuses `lower_call_args`
    // (not a bare per-argument `lower_node` map) so a trailing keyword hash
    // (`yield x: 1, y: 2`) is recognized the same way an ordinary call's
    // is -- the block-parameter binding binds a block's own
    // keyword params from the LAST *positional* yielded value, matching
    // real Ruby's auto-conversion of a trailing Hash into block keywords,
    // so the peeled kwargs are folded back into one trailing `HashLit`.
    if let Some(yield_node) = node.as_yield_node() {
        let (mut args, kwargs, _fwd_block) = lower_call_args(result, hir, yield_node.arguments())?;
        // A `*expr` splat needs no handling here: `Yield` carries the same
        // `Vec<ArrayElem>` a `Call`'s positional args do, and codegen flattens
        // a `Splat` element at runtime. Keyword args (literal pairs AND `**h`
        // double-splats) fold into one trailing `HashLit`, which codegen
        // builds via the shared `KwArg` emitter and the block-parameter
        // binding reads a block's keyword params from.
        //
        // The fold is recorded, because a hash that arrived as KEYWORDS is
        // dropped when it turns out empty at runtime while one the source wrote
        // is not -- see `NodeFlag::KWARGS_HASH`.
        if !kwargs.is_empty() {
            let hash = hir.push(HirNode::HashLit(kwargs));
            hir.set_flag(hash, crate::hir::NodeFlag::KWARGS_HASH);
            args.push(ArrayElem::Single(hash));
        }
        return Ok(Some(hir.push(HirNode::Yield(args))));
    }

    if let Some(sup) = node.as_super_node() {
        // The same argument lowering a CALL gets, for the same reasons:
        // positional args stay positional (carrying splats as
        // `ArrayElem::Splat`), a trailing keyword hash becomes `kwargs` bound
        // to the parent's keyword params by name, and `super(...)` inside
        // `def m(...)` expands to the three internal forwarding params.
        //
        // Lowering these by hand here is what made `super(...)` a gap while
        // `n(...)` worked: the hand-rolled loop reached `lower_array_elem`,
        // which has no forwarding arm, so the `...` fell through to the
        // generic rejection.
        let (args, kwargs, fwd_block) = lower_call_args(result, hir, sup.arguments())?;
        // A literal `super(x) { ... }` block vs a `super(x, &blk)` block-pass --
        // the same either/or a call carries (`BlockNode` vs `BlockArgumentNode`).
        // A `...`-forwarded block is a block-pass the arguments produced, and
        // an explicit one written beside it wins.
        let (block, block_arg) = match sup.block() {
            None => (None, fwd_block),
            Some(b) => {
                if let Some(barg) = b.as_block_argument_node() {
                    let expr = match barg.expression() {
                        Some(e) => lower_node(result, hir, &e)?,
                        None => hir.push(HirNode::LocalRead("__anon_blk".to_string())),
                    };
                    (None, Some(expr))
                } else {
                    (Some(lower_block(result, hir, &b)?), None)
                }
            }
        };
        return Ok(Some(hir.push(HirNode::SuperCall {
            args,
            kwargs,
            zsuper: false,
            block,
            block_arg,
        })));
    }

    // Bare `super` (no parens) -- a distinct prism node from `super(...)`
    // since it forwards the enclosing method's arguments implicitly (as
    // currently bound, including reassignments -- oracle-verified). The
    // `zsuper` flag carries that distinction to codegen's
    // `emit_super_arg_bindings`; see `HirNode::SuperCall`'s docs.
    if let Some(fsup) = node.as_forwarding_super_node() {
        let block = match fsup.block() {
            None => None,
            Some(b) => Some(lower_block(result, hir, &b.as_node())?),
        };
        return Ok(Some(hir.push(HirNode::SuperCall {
            args: Vec::new(),
            kwargs: Vec::new(),
            zsuper: true,
            block,
            block_arg: None,
        })));
    }

    Ok(None)
}

/// Whether a `define_method`/`define_singleton_method` block reads or writes a
/// local belonging to a scope OUTSIDE itself.
///
/// The desugar to a plain `DefMethod` turns the block into a method the class
/// owns, and a method is a compiled function of its own -- it cannot reach a local
/// living on the enclosing class-body (or top-level) frame. rubygems writes
/// exactly that shape:
///
/// ```ruby
/// module Kernel
///   original_warn = instance_method(:warn)
///   module_function define_method(:warn) { |*m, **kw| original_warn.bind_call(self, *m, **kw) }
/// end
/// ```
///
/// which emitted a method body naming `original_warn` and stopped bundler at
/// rustc with E0425. A capturing block falls through to the generic call
/// instead, where `Module#define_method` installs a real closure -- the same
/// path a `define_method` inside a method body already took, and the reason
/// that one always worked.
///
/// Prism answers this directly: a local-variable node carries the number of
/// scopes it reaches UP, and only `Block`/`Lambda` share the chain -- a
/// `def`/`class`/`module` starts a fresh one, so nothing inside it can be
/// reaching a local of ours and the walk stops there. Over-reporting is the
/// safe direction (the runtime path is correct, just less direct), so any
/// scope-maker this misses costs optimization rather than correctness.
fn closes_over_an_enclosing_local(block: &Node<'_>) -> bool {
    struct Free {
        nesting: u32,
        found: bool,
    }
    impl<'pr> ruby_prism::Visit<'pr> for Free {
        fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
            self.nesting += 1;
            ruby_prism::visit_block_node(self, node);
            self.nesting -= 1;
        }
        fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
            self.nesting += 1;
            ruby_prism::visit_lambda_node(self, node);
            self.nesting -= 1;
        }
        // A fresh scope chain: its locals are its own, and prism's depths
        // inside it are counted from there.
        fn visit_def_node(&mut self, _: &ruby_prism::DefNode<'pr>) {}
        fn visit_class_node(&mut self, _: &ruby_prism::ClassNode<'pr>) {}
        fn visit_module_node(&mut self, _: &ruby_prism::ModuleNode<'pr>) {}
        fn visit_singleton_class_node(&mut self, _: &ruby_prism::SingletonClassNode<'pr>) {}

        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
            self.found |= n.depth() >= self.nesting;
        }
        fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'pr>) {
            self.found |= n.depth() >= self.nesting;
            ruby_prism::visit_local_variable_write_node(self, n);
        }
        fn visit_local_variable_target_node(
            &mut self,
            n: &ruby_prism::LocalVariableTargetNode<'pr>,
        ) {
            self.found |= n.depth() >= self.nesting;
        }
        fn visit_local_variable_and_write_node(
            &mut self,
            n: &ruby_prism::LocalVariableAndWriteNode<'pr>,
        ) {
            self.found |= n.depth() >= self.nesting;
            ruby_prism::visit_local_variable_and_write_node(self, n);
        }
        fn visit_local_variable_or_write_node(
            &mut self,
            n: &ruby_prism::LocalVariableOrWriteNode<'pr>,
        ) {
            self.found |= n.depth() >= self.nesting;
            ruby_prism::visit_local_variable_or_write_node(self, n);
        }
        fn visit_local_variable_operator_write_node(
            &mut self,
            n: &ruby_prism::LocalVariableOperatorWriteNode<'pr>,
        ) {
            self.found |= n.depth() >= self.nesting;
            ruby_prism::visit_local_variable_operator_write_node(self, n);
        }
    }
    // Starts at 0 because the walk enters through the block ITSELF, whose
    // `visit_block_node` takes it to 1 -- so a read of the block's own local
    // (depth 0) is under the bar and a read one scope out (depth 1) is at it.
    let mut free = Free {
        nesting: 0,
        found: false,
    };
    ruby_prism::Visit::visit(&mut free, block);
    free.found
}

/// The ordinary `CallNode` arm of [`super::lower_node_inner`]: a driver over
/// the named-arm recognizers below (`new`, `define_method`, `raise`,
/// `require`, ...), then the generic `Call` fallthrough. Every path returns.
///
/// The SEQUENCE here is the contract -- see the inline notes: `using` first,
/// then the `receiver` rebind that every arm below reads, then the arms in
/// their historical order, with `lower_require_call`/`lower_autoload`
/// non-terminal on their fall-through shapes.
pub(super) fn lower_call_node(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    call: CallNode<'_>,
) -> PResult<NodeId> {
    let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();

    // `using M` -- the top-level spelling, where it activates for the
    // rest of the file. The class-body spelling is recognized by
    // `defs::lower_class_body_statement`, which reaches the same helper.
    // `using Module.new { refine C do ... end }` answers with a PAIR (the
    // anonymous holder module, then the activation), and a statement is one
    // node -- so the pair rides in a `Seq`, which the analyze walk descends
    // to register the holder exactly as it does a `ClassDef` written in any
    // other value position.
    if let Some(nodes) = defs::lower_using(result, hir, node, &name, &call)? {
        return Ok(match nodes[..] {
            [id] => id,
            _ => hir.push(HirNode::Seq(nodes)),
        });
    }

    /// Kernel's module functions that zeo answers with a COMPILE-TIME form
    /// rather than a runtime method row, so `Kernel.<name>` has to be
    /// recognized here to reach the same form. The list is
    /// `Kernel.singleton_methods(false) - Module.instance_methods` (which
    /// is what keeps Module's own `Kernel.name`/`Kernel.inspect` out),
    /// narrowed to the ones with no row.
    const KERNEL_FOLDED_FUNCTIONS: &[&str] = &[
        "__callee__",
        "__dir__",
        "__method__",
        "abort",
        "at_exit",
        "binding",
        "block_given?",
        "exec",
        "exit",
        "exit!",
        "fork",
        "gets",
        "global_variables",
        "iterator?",
        "lambda",
        "local_variables",
        "printf",
        "rand",
        "readline",
        "readlines",
        "select",
        "set_trace_func",
        "srand",
        "syscall",
        "test",
        "trace_var",
        "untrace_var",
    ];

    // `Kernel.foo(...)` -- an explicit module receiver in front of one of
    // Kernel's MODULE FUNCTIONS. Ruby defines each of them twice, as a
    // private instance method and as a singleton method on the module, and
    // both copies read the CALLER's frame: `Kernel.block_given?` and
    // `Kernel.binding` ask about the enclosing method exactly as the bare
    // spellings do. So the receiver carries no information, and dropping it
    // here lets one lowering -- and one codegen form -- serve both
    // spellings. Only the names zeo answers with a compile-time form are
    // listed; the rest already reach Kernel's own runtime row through
    // ordinary dispatch, which is the more faithful route anyway (there,
    // a user `def puts` cannot shadow `Kernel.puts`).
    let receiver = call.receiver().filter(|r| {
        !(KERNEL_FOLDED_FUNCTIONS.contains(&name.as_str())
            && r.as_constant_read_node()
                .is_some_and(|c| String::from_utf8_lossy(c.name().as_slice()) == "Kernel"))
    });

    // The arm chain, in the order the old single-function body ran it --
    // load-bearing, do not sort:
    // - every arm reads the REBOUND `receiver` above (only `lower_new_call`
    //   spells `call.receiver()`, which is identical for `new`);
    // - `lower_send_rewrite` sits before the direct `block_given?` family:
    //   it recognizes the literal-symbol `send(:block_given?)` spellings of
    //   the same caller-scope queries (historically it REBOUND `name` and
    //   fell through to them; today each recognized shape answers directly,
    //   so `name` is never rewritten);
    // - `lower_require_call` is non-terminal for a dynamic/kept target, and
    //   `lower_autoload` ALWAYS falls through: both record loader side
    //   effects, then the call reaches the generic lowering and the runtime
    //   `Kernel#require`/`Module#autoload` rows at its document position.
    if let Some(id) = lower_new_call(result, hir, &call, &name)? {
        return Ok(id);
    }
    if let Some(id) = lower_define_method(result, hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_ruby2_keywords(result, hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_define_singleton_method(result, hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_loop_call(result, hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_send_rewrite(hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_block_given(hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_dir_call(hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_local_variables(hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_lambda(result, hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_raise(result, hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    if let Some(id) = lower_require_call(result, hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    lower_autoload(hir, &call, &name, receiver.as_ref());
    if let Some(id) = lower_box_receiver(result, hir, &call, &name, receiver.as_ref())? {
        return Ok(id);
    }
    lower_generic_call(result, hir, &call, name, receiver)
}

fn lower_new_call(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
) -> PResult<Option<NodeId>> {
    // `ClassName.new(args)` -- a distinct node; see hir.rs. The
    // concurrency builtins (`Fiber.new { }`, `Thread.new { }`,
    // `Mutex.new`, `Queue.new`) are deliberately NOT this shape:
    // `Fiber`/`Thread` must keep their BLOCK (the body), which
    // `HirNode::New` has no slot for, so all four fall through to the
    // generic `Call` lowering below (receiver becomes an ordinary
    // `ClassRef(name)`) and are intercepted by the emitter's
    // builtin-constructor dispatch.
    if name == "new"
            && let Some(recv) = call.receiver()
            // `box::Widget.new(...)`: the ordinary static `New`, resolved
            // inside the box.
            && let Some((box_ctx, class_name)) = box_rooted_path(&recv)
                .map(|(bx, path)| (Some(bx), path))
                // Only a receiver that NAMES a class at compile time is a
                // static `New`. A dynamic constant scope (faraday's
                // `self.class::Handler.new`) spells a `ConstantPathNode` but
                // resolves its constant only at run time, so it joins every
                // other non-constant receiver (`x.new` on a local holding a
                // class value) on the generic `Call` lowering, which
                // dispatches via `TyKind::ClassObj`/the runtime constructor.
                .or_else(|| constant_path_name(&recv).ok().map(|n| (None, n)))
    {
        // `Enumerator.new { |y| ... }` joins the block-keeping set
        //: it falls through to the generic `Call`
        // lowering so the block reaches the runtime allocator via
        // the dynamic Class#new arm. `Proc.new { ... }` is in the
        // set for the same reason -- its block IS the value it
        // answers, and `HirNode::New` has no slot to carry one.
        // `Array.new(n) { |i| ... }` likewise: its block computes
        // each element, and routing it through `HirNode::New` would
        // silently DROP the block and answer `[nil, nil, ...]`.
        // A `*args` positional splat or `**h` double-splat can't bind
        // on the STATIC `New` path (`New.args` is `Vec<NodeId>`, no
        // runtime arg-vector, and a `**h`'s keys aren't known until
        // runtime). Fall through to the generic `Call` lowering, which
        // evaluates the constant to a `RubyValue::Class` and dispatches
        // `new` through the runtime constructor (the same path a
        // non-literal `x.new` receiver already takes).
        let has_dynamic_args = call
            .arguments()
            .map(|a| {
                a.arguments().iter().any(|n| {
                    n.as_splat_node().is_some()
                            // `Klass.new(...)` inside `def m(...)`. The
                            // forwarding expands to `*rest, **kw, &blk`, which
                            // is a runtime arg vector by definition -- exactly
                            // what the static path cannot take.
                            || n.as_forwarding_arguments_node().is_some()
                            || n.as_keyword_hash_node().is_some_and(|kw| {
                                kw.elements()
                                    .iter()
                                    .any(|e| e.as_assoc_splat_node().is_some())
                            })
                })
            })
            .unwrap_or(false);
        // A LITERAL block (`Foo.new(x) { ... }`) is captured and
        // forwarded to `initialize`; a block-PASS (`&p`) has no
        // `.as_block_node()` and falls through to the generic `Call`
        // lowering (its dynamic `new` dispatch threads the block arg).
        let block_pass = call.block().is_some_and(|b| b.as_block_node().is_none());
        if !has_dynamic_args
            && !block_pass
            && !matches!(
                // An absolute `::Proc`/`::Fiber` path names the same
                // builtin; match on the leaf so it keeps its block too.
                class_name.strip_prefix("::").unwrap_or(class_name.as_str()),
                // `Class.new(Super) { body }` keeps its block --
                // the block IS the anonymous class's body; `HirNode::New`
                // has no slot for it, so it falls through to the generic
                // `Call` and the runtime `Class#new`.
                // `Struct.new(...)` (and `Data.define`, which uses
                // `.define` and never enters this `.new` path) MINTS A
                // CLASS at runtime (`rstruct::struct_new`) in EVERY
                // position -- Batch E: whether anonymous (a local/inline
                // value) or bound to a constant (`Name = Struct.new(...)`,
                // an ordinary constant write whose value is this call).
                // It must reach the generic dynamic `new` dispatch rather
                // than a static `New`; its block is the new class's body,
                // kept the same way `Class.new`'s is.
                "Fiber"
                    | "Thread"
                    | "Mutex"
                    | "Queue"
                    | "SizedQueue"
                    | "Ractor"
                    | "Enumerator"
                    | "Proc"
                    | "Array"
                    | "Hash"
                    | "Set"
                    | "Class"
                    | "Module"
                    | "Struct"
            )
        {
            // A trailing keyword hash lands in `kwargs`, kept apart
            // from the positionals exactly as an ordinary call's is,
            // so `initialize`'s keyword params bind as keywords.
            // A callee declaring NO keyword params still sees the
            // options hash it expects --
            // `emit_call_args_to` converts trailing keywords back to
            // one positional Hash in that case, which is Ruby's own
            // rule and what keyword_init Structs bind through.
            let mut args = Vec::new();
            let mut kwargs = Vec::new();
            if let Some(a) = call.arguments() {
                for n in a.arguments().iter() {
                    if let Some(kw) = n.as_keyword_hash_node() {
                        let elements: Vec<Node<'_>> = kw.elements().iter().collect();
                        kwargs = lower_kwargs(result, hir, &elements)?;
                        continue;
                    }
                    args.push(lower_node(result, hir, &n)?);
                }
            }
            let block = match call.block() {
                Some(b) if b.as_block_node().is_some() => Some(lower_block(result, hir, &b)?),
                _ => None,
            };
            let new_id = hir.push(HirNode::New {
                class_name,
                args,
                kwargs,
                block,
            });
            return Ok(Some(match box_ctx {
                Some(bx) => hir.push(HirNode::BoxScope {
                    box_id: bx,
                    body: vec![new_id],
                }),
                None => new_id,
            }));
        }
    }
    Ok(None)
}

fn lower_define_method(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `define_method(:literal) { block }` -- desugars to a plain
    // `DefMethod`, identical treatment to `def`, mirroring zeo's
    // `walk_scope`. Every other shape -- a computed name, a body passed
    // as a value rather than written as a block, or a block that CLOSES OVER
    // an enclosing local -- falls through to the generic `Call` below and is
    // served at run time by `Module#define_method`.
    if name == "define_method"
        && receiver.is_none()
        && let (Some(args), Some(block_node)) = (call.arguments(), call.block())
    {
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() == 1
            && let Some(sym) = arg_list[0].as_symbol_node()
        {
            let method_name = String::from_utf8_lossy(sym.unescaped()).into_owned();
            // `define_method(:name, &:other)` -- a symbol-to-proc
            // block argument rather than a literal block.
            //
            // In CRuby the `&` conversion happens at the CALL SITE,
            // before `rb_mod_define_method` ever runs (proc.c:2872,
            // which rejects a bare Symbol as its second positional
            // argument), so the method body is the symbol proc:
            // `->(recv, *rest) { recv.other(*rest) }`. That is why
            // the defined method takes its RECEIVER as the first
            // argument -- `w.as_str(7)` answers `7.to_s`.
            if let Some(target) = block_node
                .as_block_argument_node()
                .and_then(|b| b.expression())
                .and_then(|e| e.as_symbol_node())
            {
                let target = String::from_utf8_lossy(target.unescaped()).into_owned();
                let recv = hir.push(HirNode::LocalRead("__sp_recv".to_string()));
                let rest = hir.push(HirNode::LocalRead("__sp_args".to_string()));
                let call = hir.push(HirNode::Call {
                    receiver: Some(recv),
                    name: target,
                    args: vec![ArrayElem::Splat(rest)],
                    kwargs: Vec::new(),
                    block: None,
                    block_arg: None,
                    safe: false,
                });
                return Ok(Some(hir.push(HirNode::DefMethod {
                    name: method_name,
                    params: Box::new(Params {
                        required: vec!["__sp_recv".to_string()],
                        rest: Some(Some("__sp_args".to_string())),
                        ..Params::default()
                    }),
                    body: vec![call],
                    is_class_method: false,
                    visibility: Visibility::Public,
                    // An explicit `define_method` call, not a `def`.
                    is_def: false,
                })));
            }
            // `define_method(:name, &proc_expr)` -- the body is a value the
            // program computes, so there is no source to desugar into a
            // `def`. Fall through to the ordinary call, which reaches
            // `Module#define_method` in the runtime; that row installs a
            // Proc, a Method or an UnboundMethod body and exists for
            // exactly the shapes this desugar cannot take.
            // Declined only inside a `class`/`module` body. A method body
            // never needed it (the def is already emitted as a closure there),
            // and at the TOP LEVEL the generic call would dispatch
            // `define_method` on `main`, which zeo's runtime has no row for --
            // trading a compile error for a NoMethodError, which is the wrong
            // direction. That one stays a gap.
            if let Some(block) = block_node.as_block_node()
                && !(hir.enclosing_class().is_some()
                    && !hir.is_in_def_body()
                    && closes_over_an_enclosing_local(&block_node))
            {
                let params = match block.parameters() {
                    None => Params::default(),
                    Some(p) => {
                        let bp = p
                            .as_block_parameters_node()
                            .ok_or("unsupported block parameter form")?;
                        lower_params(result, hir, bp.parameters())?
                    }
                };
                let body = lower_body(result, hir, block.body())?;
                let id = hir.push(HirNode::DefMethod {
                    name: method_name,
                    params: Box::new(params),
                    body,
                    is_class_method: false,
                    visibility: Visibility::Public,
                    // An explicit `define_method` call, not a `def`.
                    is_def: false,
                });
                hir.set_flag(id, crate::hir::NodeFlag::BLOCK_BODIED_DEF);
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

fn lower_ruby2_keywords(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `ruby2_keywords def fwd(*a)` written at the TOP LEVEL, where the
    // directive is a PRIVATE SINGLETON method of `main` -- an object zeo
    // dispatches by name, with no such row. The def is answered in the call's
    // place, carrying the mark `analyze::mark_ruby2_keywords_defs` sets for
    // every other position (where `Module#ruby2_keywords` is a real row and
    // the statement must stay put -- consuming it there changed how the
    // class-body walk saw the body, and delegate.rb's `method_missing` stopped
    // reaching WeakRef's instances).
    if name == "ruby2_keywords"
        && receiver.is_none()
        && hir.enclosing_class().is_none()
        && let Some(args) = call.arguments()
    {
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if let [only] = arg_list.as_slice()
            && only.as_def_node().is_some()
        {
            let id = lower_node(result, hir, only)?;
            hir.set_flag(id, crate::hir::NodeFlag::RUBY2_KEYWORDS);
            return Ok(Some(id));
        }
    }
    Ok(None)
}

fn lower_define_singleton_method(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `define_singleton_method(:literal) { block }` -- desugars to a
    // `def self.name` on the target class. The target comes from the
    // receiver: none / `self` (inside a class body) means the enclosing
    // class, so a bare `DefMethod { is_class_method: true }` lands in the
    // current body and registers there; a literal-constant / constant-
    // path receiver (`C.` / `M::D.`) reopens that named class with an
    // inline `ClassDef`. A computed name, a computed receiver, or a
    // capturing block that this desugar can't model falls through to the
    // generic (unsupported) `Call`.
    // `&callable` is NOT a literal block: `define_singleton_method(:now,
    // &block)` hands over a Proc the caller already holds, which this
    // compile-time desugar has no body to install. The runtime row takes
    // it (`Kernel#define_singleton_method` accepts a block ARGUMENT and a
    // Method/Proc positional alike), so fall through rather than refuse --
    // ddtrace, mcp and datasource all write it that way.
    // ...and never INSIDE a `def`'s body: the constant-receiver desugar
    // below mints a `ClassDef` marker, and the analyze walk registers no
    // class-body site in a method body (ruby itself rejects a `class`
    // keyword there), so the marker died in codegen as "a position the
    // analyze walk doesn't register". The generic runtime call is the
    // honest form -- `Object.define_singleton_method(:const_missing) { }`
    // inside rails_admin's suppressor method installs through the overlay,
    // and `collect_patch_call` already de-optimizes the name's call sites.
    if name == "define_singleton_method"
        && !hir.is_in_def_body()
        && let (Some(args), Some(block_node)) = (call.arguments(), call.block())
        && block_node.as_block_node().is_some()
    {
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if let (1, Some(sym)) = (
            arg_list.len(),
            arg_list.first().and_then(|a| a.as_symbol_node()),
        ) {
            let target = match receiver {
                None => Some(None),
                Some(r) if r.as_self_node().is_some() => Some(None),
                // A constant receiver reopens that named class -- but
                // ONLY when the constant actually names one. A constant
                // the program ASSIGNS (`B = Box.new`, or even `Foo =
                // Class.new`) holds a value, not a compile-time class,
                // and reopening it here would mint a bogus empty class
                // named `B`: `B.class` then answered `Class` and the
                // installed method's `self` was that phantom class, so
                // its body couldn't reach the real object's methods.
                // Those fall through to the generic runtime
                // `define_singleton_method` call, which installs a
                // per-object singleton correctly.
                //
                // ...and only when the call sits in a CLASS BODY. An explicit
                // constant receiver at STATEMENT position
                // (`Later.define_singleton_method(:mode) { }` after a call to
                // `Later.mode`) is a runtime event with a document position:
                // desugaring it to a reopen made last-`def`-wins answer the
                // new body at BOTH sites, so the method reached back in time.
                // The runtime row installs into the overlay at the right
                // moment, and `collect_patch_call` has already de-optimized
                // the name's call sites.
                Some(_) if hir.enclosing_class().is_none() => None,
                Some(r) => constant_path_name(r)
                    .ok()
                    .filter(|n| !const_is_assigned(hir, n))
                    .map(Some),
            };
            if let Some(target) = target {
                let method_name = String::from_utf8_lossy(sym.unescaped()).into_owned();
                let block = block_node
                    .as_block_node()
                    .ok_or("define_singleton_method's argument must be a block")?;
                let params = match block.parameters() {
                    None => Params::default(),
                    Some(p) => {
                        let bp = p
                            .as_block_parameters_node()
                            .ok_or("unsupported block parameter form")?;
                        lower_params(result, hir, bp.parameters())?
                    }
                };
                let body = lower_body(result, hir, block.body())?;
                let def = hir.push(HirNode::DefMethod {
                    name: method_name,
                    params: Box::new(params),
                    body,
                    is_class_method: true,
                    visibility: Visibility::Public,
                    // A `define_singleton_method` CALL, not a `def` -- the
                    // same value its `define_method` sibling above carries,
                    // and three separate behaviours read it. The body is a
                    // CLOSURE, so the capture walk must descend into it
                    // (`captures`'s `DefMethod` arm skips a real `def`,
                    // which is a scope of its own); a bare `super` in it
                    // raises ruby's "implicit argument passing of super
                    // from method defined by define_method()" instead of
                    // running (`scope_is_define_method`); and its frame is
                    // labelled after where the BLOCK was written --
                    // `block in <class:E>`, not `E.trace`.
                    is_def: false,
                });
                hir.set_flag(def, crate::hir::NodeFlag::BLOCK_BODIED_DEF);
                return Ok(Some(match target {
                    None => def,
                    Some(class_name) => hir.push(HirNode::ClassDef {
                        name: class_name,
                        superclass: None,
                        body: vec![def],
                        is_module: false,
                    }),
                }));
            }
        }
    }
    Ok(None)
}

fn lower_loop_call(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `loop do ... end` -- `Kernel#loop` is an ordinary method call, not
    // syntax, so this is a lowering-time call-shape desugar exactly like
    // `define_method` above, not a distinct `ruby-prism` node. Only a
    // zero-arg, no-param-block `loop` desugars here; anything else (an
    // explicit receiver, arguments, or declared block params -- which
    // `Kernel#loop` never yields anyway) falls through to the generic
    // `Call` case and is handled as an ordinary (currently unsupported)
    // implicit-self call.
    if name == "loop" && receiver.is_none() {
        let no_args = call
            .arguments()
            .is_none_or(|a| a.arguments().iter().next().is_none());
        if no_args
            && let Some(block_node) = call.block()
            && let Some(block) = block_node.as_block_node()
        {
            let has_params = block
                .parameters()
                .is_some_and(|p| p.as_block_parameters_node().is_some());
            if !has_params {
                let body = lower_body(result, hir, block.body())?;
                // `Kernel#loop`'s REAL definition (CRuby
                // kernel.rb:151) rescues StopIteration and
                // returns its `result` -- desugared here into
                // the ordinary Begin/rescue machinery, so
                // `loop { e.next }` terminates
                // cleanly with the enumeration's result and a
                // manual `raise StopIteration` returns nil.
                let native_loop = hir.push(HirNode::Loop { body });
                // A FRESH binding per `loop`: a fixed name aliased every
                // `loop` in the program to one local, so a scope's own
                // desugar looked like a capture of the enclosing scope's
                // -- `Ractor.new { loop { ... } }` inside a method that
                // also used `loop` refused isolation over it.
                let stop_name = hir.gensym("__loop");
                let exc_read = hir.push(HirNode::LocalRead(stop_name.clone()));
                let result_call = hir.push(HirNode::Call {
                    receiver: Some(exc_read),
                    name: "result".to_string(),
                    args: Vec::new(),
                    kwargs: Vec::new(),
                    block: None,
                    block_arg: None,
                    safe: false,
                });
                return Ok(Some(hir.push(HirNode::Begin {
                    body: vec![native_loop],
                    rescues: vec![crate::hir::RescueClause {
                        classes: vec!["StopIteration".to_string()],
                        splats: Vec::new(),
                        binding: Some(stop_name),
                        body: vec![result_call],
                    }],
                    else_body: None,
                    ensure_body: None,
                })));
            }
        }
    }
    Ok(None)
}

fn lower_send_rewrite(
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `send(:block_given?)` / `send(:binding)` / `send(:iterator?)` /
    // `send(:local_variables)` with a LITERAL symbol is the reflective
    // spelling of the same caller-scope query, so it folds exactly like
    // the direct one (the `is_sent_eval` precedent) -- rewritten here to
    // the direct name so every recognizer below, and the Binding cell
    // promotion in `analyze::captures`, sees the plain spelling.
    // `public_send` is excluded: all four are private, so CRuby raises
    // NoMethodError there. A COMPUTED name stays a runtime send and
    // reaches the loud refusal rows (see
    // tests/gaps/kernel_scope_intrinsics_dynamic_send.rb).
    if !(matches!(name, "send" | "__send__") && receiver.is_none() && call.block().is_none()) {
        return Ok(None);
    }
    {
        let sent: Vec<_> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        let target = sent
            .first()
            .and_then(|n| n.as_symbol_node())
            .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned());
        match target.as_deref() {
            Some(t @ ("block_given?" | "iterator?" | "binding" | "local_variables"))
                if sent.len() == 1 =>
            {
                match t {
                    "block_given?" | "iterator?" => {
                        return Ok(Some(hir.push(HirNode::BlockGiven)));
                    }
                    "binding" => {
                        return Ok(Some(hir.push(HirNode::Call {
                            receiver: None,
                            name: "binding".to_string(),
                            args: vec![],
                            kwargs: vec![],
                            block: None,
                            block_arg: None,
                            safe: false,
                        })));
                    }
                    _ => {
                        let binding = hir.push(HirNode::Call {
                            receiver: None,
                            name: "binding".to_string(),
                            args: vec![],
                            kwargs: vec![],
                            block: None,
                            block_arg: None,
                            safe: false,
                        });
                        return Ok(Some(hir.push(HirNode::Call {
                            receiver: Some(binding),
                            name: "local_variables".to_string(),
                            args: vec![],
                            kwargs: vec![],
                            block: None,
                            block_arg: None,
                            safe: false,
                        })));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

fn lower_block_given(
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `block_given?` -- an ordinary zero-arg `Kernel` method call at the
    // `ruby-prism` level (not a distinct node, unlike `yield` above), so
    // this is a lowering-time call-shape desugar exactly like
    // `loop`/`define_method`. An explicit `self` receiver
    // (`self.block_given?`) is the same query about the current method's
    // block, so it desugars identically. `iterator?` is CRuby's (deprecated)
    // alias for `block_given?` and folds the same way.
    let bg_self_or_none = match receiver {
        None => true,
        Some(r) => r.as_self_node().is_some(),
    };
    if (name == "block_given?" || name == "iterator?") && bg_self_or_none {
        let no_args = call
            .arguments()
            .is_none_or(|a| a.arguments().iter().next().is_none());
        if no_args && call.block().is_none() {
            return Ok(Some(hir.push(HirNode::BlockGiven)));
        }
    }
    Ok(None)
}

fn lower_dir_call(
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `__dir__` -- a `Kernel` METHOD (not a keyword like `__FILE__`), but
    // one whose answer is fixed by where it was written, so it folds to
    // the same kind of literal. Defined as
    // `File.dirname(File.realpath(__FILE__))`, oracle-verified:
    // `__dir__ == File.dirname(File.expand_path(__FILE__))`.
    //
    // Folded rather than implemented as a runtime row, because a runtime
    // one could only ever answer the MAIN file's directory -- by then
    // every required file's statements share one `Program` and the
    // authorship is gone. That would be silently wrong for a `__dir__`
    // inside a required file, which is the main reason to write one.
    if name == "__dir__" && receiver.is_none() {
        let no_args = call
            .arguments()
            .is_none_or(|a| a.arguments().iter().next().is_none());
        if no_args && call.block().is_none() {
            let dir = current_dir_str()?;
            return Ok(Some(hir.push(HirNode::StringLit(vec![StrPart::Lit(dir)]))));
        }
    }
    Ok(None)
}

fn lower_local_variables(
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `local_variables` -- the names in scope where the call is written.
    // A Binding of this scope already carries exactly those, in exactly
    // that order, so this desugars to `binding.local_variables` and
    // inherits the whole Binding machinery, the analysis that promotes
    // those locals to shared cells included.
    if name == "local_variables" && receiver.is_none() && call.block().is_none() {
        let no_args = call
            .arguments()
            .is_none_or(|a| a.arguments().iter().next().is_none());
        if no_args {
            let binding = hir.push(HirNode::Call {
                receiver: None,
                name: "binding".to_string(),
                args: vec![],
                kwargs: vec![],
                block: None,
                block_arg: None,
                safe: false,
            });
            return Ok(Some(hir.push(HirNode::Call {
                receiver: Some(binding),
                name: "local_variables".to_string(),
                args: vec![],
                kwargs: vec![],
                block: None,
                block_arg: None,
                safe: false,
            })));
        }
    }
    Ok(None)
}

fn lower_lambda(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `lambda { ... }` / `lambda do ... end` -- an alternate spelling of
    // `-> { ... }` (an ordinary `Kernel` method call with a block, not a
    // distinct node, unlike `LambdaNode` above) -- same call-shape
    // desugar posture as `loop`/`block_given?`. Only a zero-arg,
    // literal-block call desugars here; anything else (an explicit
    // receiver, arguments, or a forwarded `&block`) falls through to an
    // ordinary `Call`, a clean rejection at codegen if `lambda` itself
    // isn't otherwise defined (matching `loop`'s identical posture).
    if name == "lambda" && receiver.is_none() {
        let no_args = call
            .arguments()
            .is_none_or(|a| a.arguments().iter().next().is_none());
        if no_args
            && let Some(block_node) = call.block()
            && let Some(block) = block_node.as_block_node()
        {
            let params = lower_block_like_params(result, hir, block.parameters())?;
            let body = lower_body(result, hir, block.body())?;
            return Ok(Some(hir.push(HirNode::Lambda {
                params: Box::new(params),
                body,
                method_body: false,
            })));
        }
    }
    Ok(None)
}

fn lower_raise(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `raise` -- a zero/one/two positional-arg call-shape desugar, same
    // posture as `block_given?` above. The `cause:` keyword form isn't
    // lowered yet (see `HirNode::Raise`'s docs) -- rejected here rather
    // than silently dropped, matching this project's "clean rejection
    // over silent wrongness" rule.
    // `fail` is NOT desugared here even though it is `raise`'s exact
    // synonym: it is also a popular USER method name (riot's reporter
    // takes four arguments), and a parse-time desugar binds the Kernel
    // meaning before method resolution can see the user's `def fail`.
    // It lowers as an ordinary call and gains its raise meaning in
    // codegen's universal implicit forms, after sibling resolution.
    // `raise(*exc)` -- a splat arg has no static positional shape (its count
    // is a runtime value), so the special static-form lowering can't build
    // `HirNode::Raise`'s fixed 0..3 args. Skip it here; the general call
    // lowering handles it via `emit_splat_call` over the runtime
    // `Kernel#raise` builtin (`optparse.rb`'s `{|*exc| raise(*exc)}`).
    // `raise(...)` -- argument forwarding is the same no-static-shape
    // case as the splat (sus forwards a matcher's whole failure into
    // `raise`); the general call lowering expands `...` into
    // `*rest, **kw, &blk` and reaches the runtime row.
    let raise_has_splat = || {
        call.arguments().is_some_and(|a| {
            a.arguments()
                .iter()
                .any(|n| n.as_splat_node().is_some() || n.as_forwarding_arguments_node().is_some())
        })
    };
    if name == "raise" && receiver.is_none() && !raise_has_splat() {
        let arg_list: Vec<_> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        // A trailing keyword hash carries `cause:`. Splitting it off the
        // positional list is what keeps the three-state distinction: an
        // ABSENT `cause:` chains from `$!`, while `cause: nil` is
        // `Explicit` with a nil value and suppresses chaining.
        let (kw_nodes, positional): (Vec<_>, Vec<_>) = arg_list
            .iter()
            .partition(|n| n.as_keyword_hash_node().is_some());
        // `cause:` is the ONE keyword `raise` reads. Every other one is not
        // a keyword at all: ruby collapses the rest into a single Hash and
        // hands it over as an ordinary positional argument, so
        // `raise NotAuthorizedError, query: q, record: r` (pundit) is
        // `raise NotAuthorizedError, {query: q, record: r}` and reaches
        // `NotAuthorizedError.exception(hash)`. With nothing left after
        // `cause:` is taken out, no extra positional is passed at all.
        let mut cause = RaiseCause::Absent;
        let mut rest: Vec<crate::hir::KwArg> = Vec::new();
        for kw in &kw_nodes {
            let hash = kw.as_keyword_hash_node().expect("partitioned on this");
            for element in hash.elements().iter() {
                let Some(assoc) = element.as_assoc_node() else {
                    // `**h` -- part of the collapsed hash like any pair.
                    let splat = element
                        .as_assoc_splat_node()
                        .ok_or("unsupported element in a `raise` keyword list")?;
                    let value = splat.value().ok_or("`**` needs a hash to splat")?;
                    rest.push(crate::hir::KwArg::DoubleSplat(lower_node(
                        result, hir, &value,
                    )?));
                    continue;
                };
                let key = assoc
                    .key()
                    .as_symbol_node()
                    .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
                    .unwrap_or_default();
                if key == "cause" {
                    cause = RaiseCause::Explicit(lower_node(result, hir, &assoc.value())?);
                    continue;
                }
                let k = lower_node(result, hir, &assoc.key())?;
                let v = lower_node(result, hir, &assoc.value())?;
                rest.push(crate::hir::KwArg::Pair(k, v));
            }
        }
        let collapsed = usize::from(!rest.is_empty());
        // CRuby's `Kernel#raise` rejects a bad shape at RUNTIME: the call
        // evaluates its arguments, then raises `ArgumentError`, and only
        // when execution is reached (appnexusapi passes two messages inside
        // a `rescue` arm that may never fire). A compile error here would
        // reject a program CRuby loads fine.
        let runtime_argument_error =
            |result: &ParseResult, hir: &mut Hir, msg: String| -> PResult<NodeId> {
                let mut stmts = positional
                    .iter()
                    .map(|n| lower_node(result, hir, n))
                    .collect::<PResult<Vec<_>>>()?;
                if !rest.is_empty() {
                    stmts.push(hir.push(HirNode::HashLit(rest.clone())));
                }
                if let RaiseCause::Explicit(v) = &cause {
                    stmts.push(*v);
                }
                let class_ref = hir.push(HirNode::ClassRef("ArgumentError".to_string()));
                let message = hir.push(HirNode::StringLit(vec![StrPart::Lit(msg)]));
                stmts.push(hir.push(HirNode::Raise(vec![class_ref, message], RaiseCause::Absent)));
                Ok(hir.push(HirNode::Seq(stmts)))
            };
        if positional.len() + collapsed > 3 {
            let msg = format!(
                "wrong number of arguments (given {}, expected 0..3)",
                positional.len() + collapsed
            );
            return runtime_argument_error(result, hir, msg).map(Some);
        }
        if positional.is_empty() && collapsed == 0 && matches!(cause, RaiseCause::Explicit(_)) {
            return runtime_argument_error(
                result,
                hir,
                "only cause is given with no arguments".to_string(),
            )
            .map(Some);
        }
        let mut args = positional
            .iter()
            .map(|n| lower_node(result, hir, n))
            .collect::<PResult<Vec<_>>>()?;
        if !rest.is_empty() {
            args.push(hir.push(HirNode::HashLit(rest)));
        }
        return Ok(Some(hir.push(HirNode::Raise(args, cause))));
    }
    Ok(None)
}

fn lower_require_call(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `require`/`require_relative`/`load` reaching THIS function means
    // the statement was NOT in direct top-level statement position (the
    // one place `parse::loader`'s file-level loop recognizes and resolves
    // them) -- a method body, a `begin` block, a conditional, an `eval`
    // body, a class body. A LITERAL, RESOLVABLE `require`/`require_relative`
    // folds to its load-result bool (its target was already spliced by the
    // loader's pre-pass, or a builtin feature activated). Everything else --
    // `load`, a non-literal target, or a plain `require` the loader's
    // resolvability pre-scan marked UNRESOLVABLE -- falls through to the
    // runtime `Kernel#{require,load}` below, which raises CRuby's `LoadError`
    // if and when it executes. That is what makes the optional-dependency
    // idiom (`begin; require "x"; rescue LoadError`) behave at runtime
    // exactly as in CRuby, rather than a compile error.
    if receiver.is_none() && matches!(name, "require" | "require_relative" | "load") {
        // A non-top-level `require`/`require_relative` of a LITERAL feature
        // is usually a compile-time no-op: the loader's eager pre-pass
        // (`Loader::lower_file_statements`) already spliced the target, so
        // the CALL only reports a load result.
        //
        // A native builtin has nothing to splice; its whole effect is to
        // activate a gated feature, which is a compile-time act from any
        // position. `activated_features` doubles as CRuby's loaded-features
        // table, so `require` folds to the bool `insert` reports
        // (`load.c:1413`: true the first time, false thereafter). A spliced
        // file folds to `true`: it loads at program start, so the `unless
        // defined?`/`if <cond>` guards around these requires short-circuit
        // and the return value is rarely read.
        //
        // Two kinds of require are NOT spliced and must keep their call: a
        // plain `require` the loader could not resolve, and one only a
        // method body reaches (`LoaderState::deferred_requires`). Both fall through
        // to the runtime `Kernel#require`, which answers `false` for an
        // already-loaded feature and raises `LoadError` otherwise.
        if matches!(name, "require" | "require_relative") {
            if let Some(feature) = single_literal_string_arg(result, hir, &call)? {
                // A DUAL-HOMED feature -- one that is both a gated builtin and
                // a vendored gem -- must not fold when the loader kept its
                // call. `tmpdir` is the shape: the ext half supplies the
                // constants, the GEM half supplies `Dir::Tmpname`, and the
                // loader had already demanded that half as its own unit and
                // recorded a site so the call would survive. Folding here
                // erased the call and the unit never ran.
                //
                // The ext half still activates, positionally and
                // idempotently, so the gated constants resolve inside the
                // unit body; the call then stands as a real runtime require
                // that loads the gem half and does its own `$LOADED_FEATURES`
                // bookkeeping -- recording the GEM file's path, which is what
                // CRuby lists.
                // Only where the loader ALREADY kept the call: a top-level
                // require of a dual-homed feature is spliced normally and
                // must still fold, or the same file loads twice and the
                // second require answers true where ruby says false.
                let kept_by_loader = hir.loader.dual_homed_requires.contains(&feature)
                    && (hir.loader.deferred_requires.contains(&feature)
                        || hir.lowering_file.is_some_and(|file| {
                            let key = (file, call.location().start_offset() as u32);
                            hir.loader.optional_require_sites.contains(&key)
                                || hir.loader.conditional_require_sites.contains(&key)
                        }));
                if name == "require" && features::is_builtin_feature(&feature) && !kept_by_loader {
                    let canonical = features::canonical_ext_feature(&feature).to_string();
                    let newly_loaded = hir.activated_features.insert(canonical.clone());
                    let first = newly_loaded && !features::is_preloaded_at_boot(&feature);
                    // The activation itself is positional wherever the require
                    // is written (`Loader::activate_static_ext` is only the
                    // TOP-LEVEL half of this rule), so the marker rides ahead
                    // of the load result. It is idempotent, so a second
                    // require of one feature still records nothing.
                    let marker = hir.push(HirNode::FeatureLoaded {
                        entry: format!("<zeo-builtin>/{canonical}.rb"),
                        feature: Some(canonical),
                    });
                    let value = hir.push(HirNode::BoolLit(first));
                    return Ok(Some(hir.push(HirNode::Seq(vec![marker, value]))));
                }
                let unresolvable =
                    name == "require" && hir.loader.unresolvable_requires.contains(&feature);
                // A rescued-and-missing `require_relative` keeps its call
                // to raise the LoadError its rescue catches; a
                // guard-gated site keeps its call to load its unit only
                // when the guard passes.
                // A SNIPPET splices no `require_relative`: it has no
                // compile-time file to resolve against, so the loader leaves
                // the call for `dynamic_require_relative`, which resolves it
                // against the CALLING file the frame carries. Folding to
                // `true` here would have been a lie about a load that never
                // happened. (The two conditions agree by construction: an
                // eval compile never carries an `input_path`, which is the
                // only thing that gives the loader a directory.)
                let site_kept = (hir.mode.is_eval() && name == "require_relative")
                    || hir.lowering_file.is_some_and(|file| {
                        let key = (file, call.location().start_offset() as u32);
                        (name == "require_relative"
                            && hir.loader.optional_require_sites.contains(&key))
                            || hir.loader.conditional_require_sites.contains(&key)
                    });
                // A KEPT `require_relative` resolves at RUN time against the
                // calling file, which the frame carries -- and a spliced
                // file's top-level statement has no frame of its own, so the
                // frame names the main script and the target resolves beside
                // the wrong file. The compiler knows the right directory
                // here, so it writes the absolute spelling in: an absolute
                // argument makes the run-time resolution a plain `require`,
                // which finds the unit under the name it registered.
                if site_kept
                    && name == "require_relative"
                    && !feature.starts_with('/')
                    && let Some(dir) = hir.lowering_dir.clone()
                {
                    let abs = crate::parse::absolutize_feature(&dir, &feature);
                    let arg = hir.push(HirNode::StringLit(vec![crate::hir::StrPart::Lit(abs)]));
                    return Ok(Some(hir.push(HirNode::Call {
                        receiver: None,
                        name: "require_relative".to_string(),
                        args: vec![crate::hir::ArrayElem::Single(arg)],
                        kwargs: Vec::new(),
                        block: None,
                        block_arg: None,
                        safe: false,
                    })));
                }
                if !unresolvable && !site_kept && !hir.loader.deferred_requires.contains(&feature) {
                    // FALSE when the loader marked this site as naming a file
                    // it had already spliced -- ruby's answer for a feature
                    // that is already loaded.
                    let again = hir.lowering_file.is_some_and(|file| {
                        hir.loader
                            .rerequire_sites
                            .contains(&(file, call.location().start_offset() as u32))
                    });
                    return Ok(Some(hir.push(HirNode::BoolLit(!again))));
                }
            } else if name == "require_relative"
                && let Some(dir) = computed_relative_demand_dir(&call)
            {
                // A computed `require_relative` gets a BOUNDED demand, not
                // the package-wide one: either the literal directory prefix
                // its interpolation starts with, or the requiring file's
                // SIDECAR directory (`foo.rb` alongside `foo/`, ruby's
                // conventional split). This is what keeps a MAIN-file
                // loader working: the main file deliberately demands no
                // whole directory (a demander at the tree root would sweep
                // everything -- the webrick_not_bundled hazard), but a
                // bounded subtree is its own tree.
                hir.loader
                    .unit_demand
                    .insert((hir.lowering_package.clone(), dir));
            } else {
                // A COMPUTED target can name any file of the demanding
                // package -- `Dir[...].each { |t| require t }` is how a
                // test suite or a plugin registry loads itself. The
                // honest AOT answer is the one a dynamic `autoload`
                // already gets: compile the package's files in as
                // callable units, and let the runtime require resolve
                // the string the program actually builds.
                hir.demand_feature_units();
                // ...and it can name a BUILTIN just as easily -- `%w[date
                // set].each { |f| require f }` is the same idiom over
                // stdlib names, and RubyGems, Bundler and Rails all write
                // it. A gated builtin nothing requires literally registers
                // no class at all, so the run-time require loaded the
                // feature and left the constant a `NameError`. Registering
                // them all here leaves each CONCEALED, exactly as an
                // activated one already is, so the constant still starts
                // absent and the require reveals it.
                if name == "require" {
                    hir.activate_every_gated_feature();
                }
            }
        }
        // `load`, or the computed `require` above: whole-program AOT
        // can't splice a path it only learns at runtime. FALL THROUGH to
        // the ordinary implicit-self `Call` lowering below (the same
        // trick a non-literal `eval` uses), which dispatches to the
        // runtime `Kernel#{require,require_relative,load}` -- answering
        // from the compiled-in units, and raising CRuby's `LoadError`
        // otherwise. This lets a guarded dynamic load -- `load ENV["X"]
        // if ENV["X"]` -- and the `begin; require dyn; rescue LoadError`
        // idiom COMPILE, with the guard/rescue behaving at runtime.
    }
    Ok(None)
}

/// NON-TERMINAL by design: records the loader demand, then the call ALWAYS
/// falls through to the generic lowering and the runtime `autoload` row.
fn lower_autoload(hir: &mut Hir, call: &CallNode<'_>, name: &str, receiver: Option<&Node<'_>>) {
    // `autoload :Const, "feature"` -- the loader's pre-pass
    // (`Loader::lower_file_statements`) has already compiled the feature
    // file in as a LAZY unit; the runtime `autoload` row loads it when
    // the declaration executes, at its document position.
    if name == "autoload" && receiver.is_none() {
        // Always a real call. A computed target -- including the
        // one-argument form every `autoload` DSL defines over
        // `Module#autoload` -- computes its string at runtime, so its
        // lowering demands the whole load path as units instead.
        //
        // Lowering it to a no-op on the strength of `autoload_feature`
        // alone was a false pass: the pre-pass walks class/module bodies,
        // not method bodies, so a literal `autoload` inside a `def`
        // resolved here, spliced nowhere, and silently defined nothing.
        if autoload_feature(call).is_err() {
            hir.demand_feature_units();
        }
    }
}

fn lower_box_receiver(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: &str,
    receiver: Option<&Node<'_>>,
) -> PResult<Option<NodeId>> {
    // `Ruby::Box` guard rails. Everything but ALLOCATION is an ordinary
    // runtime call (the class carries real rows); an unassigned/nested
    // `.new` would allocate a box nothing could ever reference, so that
    // one shape stays a clean compile-time rejection.
    if let Some(recv) = receiver {
        if constant_path_name(recv).is_ok_and(|n| n == "Ruby::Box") && name == "new" {
            return Err(format!(
                    "`Ruby::Box.{name}` isn't supported here (zeo limitation) -- the one supported allocation shape is `box = Ruby::Box.new` as a top-level statement"
                ).into());
        }
        // Operations on a bound box handle outside their recognized
        // positions. A TOP-LEVEL `box.require` is spliced by the loader
        // (the fast, statically typed path); anywhere else it is the
        // ordinary send, which reaches the box's own run-time load path.
        // An expression-position `box.eval` is a run-time compile too.
        if let Some(lv) = recv.as_local_variable_read_node() {
            let lname = String::from_utf8_lossy(lv.name().as_slice()).into_owned();
            if let Some(bx) = context::current_box_binding(&lname)
                && name == "eval"
            {
                return lower_box_eval(hir, result, call, bx, false).map(Some);
            }
        }
    }
    Ok(None)
}

/// The generic `Call` fallthrough -- every named arm above declined.
fn lower_generic_call(
    result: &ParseResult,
    hir: &mut Hir,
    call: &CallNode<'_>,
    name: String,
    receiver: Option<Node<'_>>,
) -> PResult<NodeId> {
    let receiver = match receiver {
        None => None,
        Some(r) => Some(lower_node(result, hir, &r)?),
    };
    let (args, kwargs, fwd_block) = lower_call_args(result, hir, call.arguments())?;
    // A call's `block()` slot is one of two distinct shapes: a literal
    // `{ }`/`do..end` (`BlockNode`), or `&existing_proc` forwarding an
    // already-built Proc value onward (`BlockArgumentNode`) -- real Ruby
    // syntax forbids a call from having both, so this is a clean
    // either/or, not a "prefer one" choice. A `...` in the argument
    // list contributes its own block forwarding (`fwd_block`).
    let (block, block_arg) = match call.block() {
        None => (None, fwd_block),
        Some(b) => {
            if let Some(barg) = b.as_block_argument_node() {
                let expr = match barg.expression() {
                    Some(e) => lower_node(result, hir, &e)?,
                    // Anonymous `&` forwarding -- references the
                    // enclosing method's internally-named `&` param
                    // (see `lower_params`).
                    None => hir.push(HirNode::LocalRead("__anon_blk".to_string())),
                };
                (None, Some(expr))
            } else {
                (Some(lower_block(result, hir, &b)?), None)
            }
        }
    };
    let is_vcall = call.is_variable_call();
    let built = HirNode::Call {
        receiver,
        name,
        args,
        kwargs,
        block,
        block_arg,
        safe: call.is_safe_navigation(),
    };
    // prism flags the call it built out of assignment syntax -- `s.x = v`
    // and `s[i] = v` are both plain `CallNode`s, distinguished from an
    // explicit `s.[]=(i, v)` by nothing else.
    if call.is_attribute_write() {
        return Ok(assign::push_assignment_call(hir, built));
    }
    let node = hir.push(built);
    if is_vcall {
        hir.set_flag(node, crate::hir::NodeFlag::VCALL);
    }
    // A literal block on a re-homing call runs under the RECEIVER's
    // `self` -- see `NodeFlag::REHOMED_BLOCK`.
    if let HirNode::Call {
        receiver: Some(_),
        name,
        block: Some(b),
        ..
    } = &hir[node]
        && matches!(
            name.as_str(),
            "instance_eval"
                | "instance_exec"
                | "class_eval"
                | "class_exec"
                | "module_eval"
                | "module_exec"
        )
    {
        let b = *b;
        hir.set_flag(b, crate::hir::NodeFlag::REHOMED_BLOCK);
    }
    // A computed-name `define_method(name) { }` block IS a method body
    // at run time -- see `NodeFlag::DYNAMIC_DEFINE_METHOD_BLOCK`.
    if let HirNode::Call {
        name,
        block: Some(b),
        ..
    } = &hir[node]
        && matches!(name.as_str(), "define_method" | "define_singleton_method")
    {
        let b = *b;
        hir.set_flag(b, crate::hir::NodeFlag::DYNAMIC_DEFINE_METHOD_BLOCK);
    }
    Ok(node)
}
