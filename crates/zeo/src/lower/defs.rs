//! `def`/`class`/`module`/singleton-class lowering: parameter lists, class
//! bodies (including the runtime-class/reopen desugars), `attr_*`/
//! `private`/`public`/`protected`/`module_function`/`alias`/`undef`
//! handling, and the const-holds-a-runtime-class checks that pick between
//! the static and runtime class-lowering paths. Split out of
//! `parse/mod.rs`.

use super::assign::lower_multi_target_group;
use super::consts::constant_path_name;
use super::control::static_bool;
use super::ffi::{
    as_ffi_layout, as_global_ffi_typedef, extend_target_path, ffi_extender_hook,
    is_extend_ffi_library, lower_ffi_directive, synthesize_ffi_struct,
};
use super::{
    PResult, lower_body, lower_node, names_enclosing_class, parse_and_lower_into, superclass_name,
};
use crate::compiler::SINGLETON_SURROGATE;
use crate::hir::{ArrayElem, Hir, HirNode, KeywordParam, NodeId, Params, StrPart, Visibility};
use ruby_prism::{Node, ParseResult};

mod def_guards;
mod directives;
mod runtime_class;
mod singleton;
pub(crate) use def_guards::*;
pub(crate) use runtime_class::*;
pub(crate) use singleton::*;

/// Lowers the branch a statically-folded class-body `if`/`unless` selected --
/// a `StatementsNode` (the `then`/`unless` body), an `ElseNode` (a final
/// `else`), a nested `IfNode` (an `elsif`, re-entering the fold), or `None`
/// (an omitted branch) -- routing each contained statement back through
/// `lower_one_class_body_stmt` so an `alias`/`def`/visibility directive
/// inside the guard still registers, and an FFI `typedef`/`ffi_lib`/
/// `attach_function` under a platform gate still reaches the FFI dispatch
/// (vips declares `:GType` under `if FFI::Platform::ADDRESS_SIZE == 64`).
fn lower_class_body_selected<'a>(
    result: &ParseResult,
    hir: &mut Hir,
    whole: &Node<'a>,
    chosen: Option<Node<'a>>,
    st: &mut LowerBodyStmt<'a>,
    out: &mut Vec<NodeId>,
) -> PResult<()> {
    bind_pruned_locals(hir, whole, chosen.as_ref(), st, out);
    let Some(node) = chosen else { return Ok(()) };
    if let Some(stmts) = node.as_statements_node() {
        for stmt in stmts.body().iter() {
            lower_one_class_body_stmt(result, hir, &stmt, st, out)?;
        }
        return Ok(());
    }
    if let Some(else_node) = node.as_else_node() {
        if let Some(stmts) = else_node.statements() {
            for stmt in stmts.body().iter() {
                lower_one_class_body_stmt(result, hir, &stmt, st, out)?;
            }
        }
        return Ok(());
    }
    // A nested `elsif` `IfNode`, or any single statement: re-enter the
    // class-body path (which folds the `elsif` in turn).
    lower_one_class_body_stmt(result, hir, &node, st, out)
}

/// Give the locals a folded-away branch would have BOUND their nil binding.
///
/// Ruby's parser creates a local for an assignment it never runs -- `if false;
/// x = 1; end; p x` answers nil, not NameError -- so pruning the dead branch
/// has to leave the binding behind. resolv.rb reads `hosts` on the line after
/// the windows-only `if` that assigns it.
///
/// A name the body already assigned EARLIER keeps its value (`x = 5; if false;
/// x = 1; end` is still 5), so those are left alone.
fn bind_pruned_locals(
    hir: &mut Hir,
    whole: &Node<'_>,
    chosen: Option<&Node<'_>>,
    st: &LowerBodyStmt<'_>,
    out: &mut Vec<NodeId>,
) {
    let mut pruned = local_writes(whole);
    if let Some(kept) = chosen {
        for name in local_writes(kept) {
            pruned.remove(&name);
        }
    }
    if pruned.is_empty() {
        return;
    }
    let before = whole.location().start_offset();
    for stmt in st.body {
        if stmt.location().start_offset() >= before {
            continue;
        }
        for name in local_writes(stmt) {
            pruned.remove(&name);
        }
    }
    for name in pruned {
        let nil = hir.push(HirNode::NilLit);
        out.push(hir.push(HirNode::LocalWrite(name, nil)));
    }
}

/// Every local name a subtree ASSIGNS into THIS scope, in any of ruby's write
/// forms. A `def`/`class`/block/lambda opens a scope of its own, so the walk
/// stops there -- a name first assigned inside one never becomes a local out
/// here. (`BTreeSet` so the emitted bindings come out in one order.)
fn local_writes(node: &Node<'_>) -> std::collections::BTreeSet<String> {
    struct Collect(std::collections::BTreeSet<String>);
    impl<'pr> ruby_prism::Visit<'pr> for Collect {
        fn visit_def_node(&mut self, _: &ruby_prism::DefNode<'pr>) {}
        fn visit_class_node(&mut self, _: &ruby_prism::ClassNode<'pr>) {}
        fn visit_module_node(&mut self, _: &ruby_prism::ModuleNode<'pr>) {}
        fn visit_singleton_class_node(&mut self, _: &ruby_prism::SingletonClassNode<'pr>) {}
        fn visit_lambda_node(&mut self, _: &ruby_prism::LambdaNode<'pr>) {}
        fn visit_block_node(&mut self, _: &ruby_prism::BlockNode<'pr>) {}
        fn visit_local_variable_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableWriteNode<'pr>,
        ) {
            self.0
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
            self.visit(&node.value());
        }
        fn visit_local_variable_target_node(
            &mut self,
            node: &ruby_prism::LocalVariableTargetNode<'pr>,
        ) {
            self.0
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
        }
        fn visit_local_variable_operator_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>,
        ) {
            self.0
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
            self.visit(&node.value());
        }
        fn visit_local_variable_and_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableAndWriteNode<'pr>,
        ) {
            self.0
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
            self.visit(&node.value());
        }
        fn visit_local_variable_or_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableOrWriteNode<'pr>,
        ) {
            self.0
                .insert(String::from_utf8_lossy(node.name().as_slice()).into_owned());
            self.visit(&node.value());
        }
    }
    use ruby_prism::Visit as _;
    let mut collect = Collect(Default::default());
    collect.visit(node);
    collect.0
}

/// `AliasMethodNode`'s `new_name`/`old_name` -- always a `SymbolNode` in
/// practice (confirmed via `Prism.parse`: both the bareword `alias new old`
/// and symbol `alias :new :old` spellings produce the identical node shape),
/// but checked defensively (a clean `Err`, not a panic) rather than assumed.
pub fn alias_target_name(node: &Node<'_>) -> PResult<String> {
    let sym = node
        .as_symbol_node()
        .ok_or("`alias`'s target must be a plain method name (zeo limitation)")?;
    Ok(String::from_utf8_lossy(sym.unescaped()).into_owned())
}

/// Registers `alias new old` / `alias_method :new, :old` into the current
/// class/module body. When `old` is defined EARLIER IN THIS SAME BODY, the
/// source `DefMethod` is cloned directly (nothing to defer -- no runtime
/// target needed). Otherwise `old` is an INHERITED method whose definition
/// isn't in this body and whose ancestry isn't linearized until `analyze`, so
/// a deferred `HirNode::AliasMethod` is emitted for `mro::resolve_aliases` to
/// resolve later. See `HirNode::AliasMethod`.
/// Turns the instance `def` at `id` into a module function: real Ruby keeps
/// BOTH halves, a public module method and a PRIVATE instance method for the
/// `include`-mixin, so the original becomes the private half and a class-method
/// copy joins it. `false` (and nothing pushed) when `id` isn't a `DefMethod`.
///
/// Whether `id` is already in `out` is the caller's business: the bare
/// `module_function` mode pushes it here, while `module_function :name` found
/// it there in the first place.
fn promote_to_module_function(hir: &mut Hir, id: NodeId, out: &mut Vec<NodeId>) -> bool {
    let HirNode::DefMethod {
        name, params, body, ..
    } = &hir[id]
    else {
        return false;
    };
    let (name, params, body) = (name.clone(), params.clone(), body.clone());
    hir.set_method_visibility(id, Visibility::Private);
    if !out.contains(&id) {
        out.push(id);
    }
    // `push_from`, not `push`: the module copy is the SAME definition, so ruby
    // reports the `def`'s own line for both halves (oracle-verified). Under a
    // plain `push` the copy inherited the enclosing module's span, which made
    // `Mod.method(:m).source_location` name the `module` line.
    out.push(hir.push_from(
        HirNode::DefMethod {
            name,
            params,
            body,
            is_class_method: true,
            visibility: Visibility::Public,
            is_def: true,
        },
        id,
    ));
    true
}

fn push_alias(hir: &mut Hir, out: &mut Vec<NodeId>, new_name: String, old_name: String) {
    if let Some(&old_id) = out
        .iter()
        .rev()
        .find(|&&id| matches!(&hir[id], HirNode::DefMethod { name, .. } if *name == old_name))
    {
        let HirNode::DefMethod {
            params,
            body,
            is_class_method,
            visibility,
            is_def,
            ..
        } = &hir[old_id]
        else {
            unreachable!("guarded by the `find` above")
        };
        let (params, body, is_class_method, visibility, is_def) = (
            params.clone(),
            body.clone(),
            *is_class_method,
            *visibility,
            *is_def,
        );
        // Carrying the SOURCE's span, not the `alias` line's: an alias
        // reports its original's `source_location`, as in CRuby.
        let cloned = hir.push_from(
            HirNode::DefMethod {
                name: new_name,
                params,
                body,
                is_class_method,
                visibility,
                is_def,
            },
            old_id,
        );
        hir.record_alias_origin(cloned, old_name);
        out.push(cloned);
    } else {
        out.push(hir.push(HirNode::AliasMethod {
            new_name,
            old_name,
            is_class_method: false,
        }));
    }
}

/// Required-parameter-only helper for a `posts`/`requireds` entry -- both
/// only ever contain `RequiredParameterNode`s (Ruby's grammar guarantees a
/// splat's "post" params are always plain required names, same as the
/// params before it).
fn required_param_name(node: &Node<'_>, where_: &str) -> PResult<String> {
    let p = node.as_required_parameter_node().ok_or_else(|| {
        format!("only plain required parameters are supported {where_} (zeo limitation)")
    })?;
    Ok(String::from_utf8_lossy(p.name().as_slice()).into_owned())
}

/// One entry of `requireds()`/`posts()`: either a plain name, or a
/// parenthesized DESTRUCTURING target list (`|a, (b, c)|`), which prism
/// surfaces as a `MultiTargetNode` in the very same slot -- the same node
/// type, with the same `lefts()`/`rest()`/`rights()` grammar, that a
/// multi-assignment's nested group uses. So it lowers through the same
/// `lower_multi_target_group`, and the slot itself gets an internal name
/// (`__destr_<i>`) that behaves as an ordinary required param everywhere
/// else -- see `Params::destructures`.
///
/// Returns the slot's name, pushing onto `destructures` when it destructures.
fn required_param_slot(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    where_: &str,
    destructures: &mut Vec<(NodeId, crate::hir::MultiTargetGroup)>,
) -> PResult<String> {
    let Some(mt) = node.as_multi_target_node() else {
        return required_param_name(node, where_);
    };
    let group = lower_multi_target_group(result, hir, mt.lefts(), mt.rest(), mt.rights())?;
    let slot = format!("__destr_{}", destructures.len());
    let read = hir.push(HirNode::LocalRead(slot.clone()));
    destructures.push((read, group));
    Ok(slot)
}

/// Full `ParametersNode` lowering: required -> optional (default evaluated
/// LAZILY by the callee -- see `Params::optional`'s docs, so its expression
/// is only lowered here, never eagerly evaluated at every call site) ->
/// rest (`*`/`*name`) -> post (required params after a splat) -> keyword
/// (required/optional) -> keyword_rest (`**`/`**name`/explicit `**nil`) ->
/// `&block`/anonymous `&` (same `None`/`Some(None)`/`Some(Some(name))` shape
/// as `rest`/`keyword_rest` -- see `hir::Params::block`'s docs). Bare `...`
/// forwarding (positional + keyword + block all at once) is a separate,
/// still-unsupported call-site construct -- see the `keyword_rest` match arm
/// below, which gives it a dedicated rejection message.
pub(crate) fn lower_params(
    result: &ParseResult,
    hir: &mut Hir,
    params: Option<ruby_prism::ParametersNode<'_>>,
) -> PResult<Params> {
    let Some(params) = params else {
        return Ok(Params::default());
    };
    // `def m(...)` -- bare forwarding. Prism surfaces it as a
    // `ForwardingParameterNode` occupying the `keyword_rest` slot (with
    // `.rest()`/`.block()` both `None`). Desugared here into three
    // compiler-internal named params (`*__fwd_rest, **__fwd_kw,
    // &__fwd_blk`); the call-site `n(...)` (a `ForwardingArgumentsNode`)
    // references the same names -- no new HIR shape, no special runtime.
    let forwarding = params
        .keyword_rest()
        .is_some_and(|n| n.as_forwarding_parameter_node().is_some());
    // Anonymous `&` (`def m(&)`) forwards via the same internal-name trick
    // (`n(&)` references it); a named `&blk` stays itself.
    let block = match params.block() {
        Some(b) => Some(Some(match b.name() {
            Some(name) => String::from_utf8_lossy(name.as_slice()).into_owned(),
            None => "__anon_blk".to_string(),
        })),
        None if forwarding => Some(Some("__fwd_blk".to_string())),
        None => None,
    };

    let mut destructures = Vec::new();
    let required = params
        .requireds()
        .iter()
        .map(|n| required_param_slot(result, hir, &n, "before a `*rest`", &mut destructures))
        .collect::<PResult<Vec<_>>>()?;

    let optional = params
        .optionals()
        .iter()
        .map(|n| {
            let p = n
                .as_optional_parameter_node()
                .ok_or("expected an optional parameter (zeo limitation)")?;
            let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
            let default = lower_node(result, hir, &p.value())?;
            Ok((name, default))
        })
        .collect::<PResult<Vec<_>>>()?;

    let implicit_rest = params
        .rest()
        .is_some_and(|n| n.as_implicit_rest_node().is_some());
    let rest = match params.rest() {
        None if forwarding => Some(Some("__fwd_rest".to_string())),
        None => None,
        // A TRAILING COMMA (`|a, |`) -- prism's `ImplicitRestNode`. It means
        // "this block takes more than one parameter", which is what turns on
        // auto-splat, and then discards everything past the named ones:
        // `m([1, 2]) { |a, | a }` is `1`, not `[1, 2]` (oracle-verified).
        // That is exactly an anonymous `*`, so it lowers as one and the
        // existing arity/auto-splat rules cover it with no special case.
        // ...and `implicit_rest` above records that it was a comma, which
        // keeps it out of the SIGNATURE (`arity`/`#parameters`) and out of a
        // lambda's strict argument count.
        Some(n) if n.as_implicit_rest_node().is_some() => Some(None),
        Some(n) => {
            let r = n
                .as_rest_parameter_node()
                .ok_or("unsupported rest-parameter form (zeo limitation)")?;
            // Anonymous `*` (`def m(*)`) gets an internal name so `n(*)`
            // can forward it (Ruby 3.2's anonymous-forwarding semantics).
            Some(Some(match r.name() {
                Some(name) => String::from_utf8_lossy(name.as_slice()).into_owned(),
                None => "__anon_rest".to_string(),
            }))
        }
    };

    let post = params
        .posts()
        .iter()
        .map(|n| required_param_slot(result, hir, &n, "after a `*rest`", &mut destructures))
        .collect::<PResult<Vec<_>>>()?;

    let keywords = params
        .keywords()
        .iter()
        .map(|n| {
            if let Some(p) = n.as_required_keyword_parameter_node() {
                Ok(KeywordParam::Required(
                    String::from_utf8_lossy(p.name().as_slice()).into_owned(),
                ))
            } else if let Some(p) = n.as_optional_keyword_parameter_node() {
                let name = String::from_utf8_lossy(p.name().as_slice()).into_owned();
                let default = lower_node(result, hir, &p.value())?;
                Ok(KeywordParam::Optional(name, default))
            } else {
                Err("unsupported keyword parameter form (zeo limitation)".into())
            }
        })
        .collect::<PResult<Vec<_>>>()?;

    // `**nil` binds nothing, so it takes no `keyword_rest` slot; what it
    // declares is recorded on `no_keywords` instead.
    let no_keywords = params
        .keyword_rest()
        .is_some_and(|n| n.as_no_keywords_parameter_node().is_some());
    let keyword_rest = match params.keyword_rest() {
        None => None,
        // Bare `...` forwarding (a `ForwardingParameterNode` in this slot)
        // -- desugared to `**__fwd_kw` here; `rest`/`block` above already
        // synthesized their `__fwd_*` halves.
        Some(n) if n.as_forwarding_parameter_node().is_some() => Some(Some("__fwd_kw".to_string())),
        Some(n) if n.as_no_keywords_parameter_node().is_some() => None,
        Some(n) => {
            let r = n
                .as_keyword_rest_parameter_node()
                .ok_or("unsupported keyword-rest parameter form (zeo limitation)")?;
            // Anonymous `**` gets an internal name so `n(**)` can forward
            // it, same as the anonymous-`*` rule above.
            Some(Some(match r.name() {
                Some(name) => String::from_utf8_lossy(name.as_slice()).into_owned(),
                None => "__anon_kwrest".to_string(),
            }))
        }
    };

    Ok(Params {
        required,
        destructures,
        optional,
        rest,
        implicit_rest,
        post,
        keywords,
        keyword_rest,
        no_keywords,
        block,
        // Filled in by `lower_block_like_params` for a block: prism keeps
        // `|x; sum|`'s locals on the BlockParametersNode, not here on the
        // ParametersNode. Always empty for a method's params -- the syntax
        // doesn't exist there.
        block_locals: Vec::new(),
        // Populated by `lower_block` from prism's block-scope local table;
        // always empty for a method's params.
        implicit_block_locals: Vec::new(),
    })
}

/// `cref` is the class/module name this body OPENS, or `None` for a body that
/// opens no cref of its own -- a `class << obj` / `class << self` (CRuby walks
/// past a singleton cref) and a runtime class body, which is an ordinary block.
/// See `Hir::in_class_body`.
/// The holder module a `refine Target do ... end` puts its methods in.
/// Deliberately unspellable as a Ruby constant, so the holder claims no
/// name inside the refining module -- `M.constants` stays what the source
/// wrote -- and it is the same name CRuby prints for `M.refinements.first`.
/// A qualified target flattens (`Foo::Bar` -> `Foo.Bar`) so the name reads
/// as one leaf rather than a nested path.
pub(crate) fn refinement_holder_name(target: &str) -> String {
    format!("#refinement:{}", target.replace("::", "."))
}

/// The `(target constant, refines-the-singleton)` a `refine` argument names,
/// or `None` for a genuinely computed one. Three spellings resolve:
/// a constant path (`String`, `CR::Season`, `::Array`), a constant's
/// `.singleton_class` (aixm's `refine Range.singleton_class` -- the holder
/// refines Range's CLASS methods), and a constant's `.class`, which for a
/// class-valued constant IS `Class` (acpc_table_manager's
/// `refine Time.class()`).
fn refine_target(node: &Node<'_>) -> Option<(String, bool)> {
    if let Ok(path) = constant_path_name(node) {
        return Some((path, false));
    }
    let call = node.as_call_node()?;
    if call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    let recv = call.receiver()?;
    let target = constant_path_name(&recv).ok()?;
    match call.name().as_slice() {
        b"singleton_class" => Some((target, true)),
        // `Time.class` reads as Class only because `Time` is itself a class;
        // the constant requirement keeps an arbitrary value's `.class` (a
        // genuinely runtime question) out.
        b"class" => Some(("Class".to_string(), false)),
        _ => None,
    }
}

/// `using M` in any position: `Some(node)` once the shape matched -- no
/// receiver, one bare constant argument. Anything else answers `None` and
/// falls through to an ordinary call, which is a clean rejection later if
/// nothing else defines `using`.
pub(crate) fn lower_using(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    name: &str,
    call: &ruby_prism::CallNode<'_>,
) -> PResult<Option<Vec<NodeId>>> {
    if name != "using" || call.receiver().is_some() {
        return Ok(None);
    }
    let arg_list: Vec<_> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let [arg] = arg_list.as_slice() else {
        return Ok(None);
    };
    if let Ok(module) = constant_path_name(arg) {
        hir.push_span(crate::lower::span_of(hir, node));
        let id = hir.push(HirNode::Using(module));
        hir.pop_span();
        return Ok(Some(vec![id]));
    }
    // `using Module.new { refine C do ... end }` -- irb's shape, and the only
    // way to activate a refinement over a class chosen where no constant
    // names the module. The anonymous module becomes an ordinary
    // compile-time module under an unwritable `#using:`-prefixed name (the
    // refinement-holder convention: claims no constant, `Module#name` stays
    // nil), so the whole existing rewrite -- holder registration, byte-range
    // activation, refined call sites -- applies unchanged.
    if let Some(mcall) = arg.as_call_node()
        && String::from_utf8_lossy(mcall.name().as_slice()) == "new"
        && mcall
            .receiver()
            .and_then(|r| {
                r.as_constant_read_node()
                    .map(|c| c.name().as_slice().to_vec())
            })
            .is_some_and(|n| n == b"Module")
        && let Some(block) = mcall.block().and_then(|b| b.as_block_node())
    {
        let span = crate::lower::span_of(hir, node);
        let anon = format!("#using:{}", span.start);
        let body = lower_class_body(result, hir, block.body(), None, Some(&anon))?;
        hir.push_span(span);
        let def = hir.push(HirNode::ClassDef {
            name: anon.clone(),
            superclass: None,
            body,
            is_module: true,
        });
        let using = hir.push(HirNode::Using(anon));
        hir.pop_span();
        return Ok(Some(vec![def, using]));
    }
    Ok(None)
}

pub(crate) fn lower_class_body(
    result: &ParseResult,
    hir: &mut Hir,
    body: Option<Node<'_>>,
    superclass: Option<&str>,
    cref: Option<&str>,
) -> PResult<Vec<NodeId>> {
    // A `class T < FFI::Struct` turns its `layout` directive into
    // synthesized `[]`/`[]=`/`size`/`offset_of`/`pointer` methods over an
    // `FFI::MemoryPointer` ivar -- see `synthesize_ffi_struct`.
    // `FFI::Union` is the same synthesis with every field at offset 0 -- see
    // `synthesize_ffi_struct`. sassc's `SassValue < FFI::Union` is the case.
    // Both anchorings: `consts::constant_path_name` keeps a leading `::` for
    // a root-anchored path, and `class T < ::FFI::Struct` is the same class.
    // `FFI::ManagedStruct` adds an auto-release finalizer over the same
    // layout, which is a lifetime concern rather than an ABI one.
    let is_ffi_union = matches!(superclass, Some("FFI::Union" | "::FFI::Union"));
    // A subclass of a struct class IS a struct class -- ruby-ffi inherits the
    // layout, and a gem that wraps `FFI::Struct` once and declares everything
    // against the wrapper (gssapi's `GssUMStruct`) never names `FFI::Struct`
    // again.
    let inherits_struct = superclass.is_some_and(|s| {
        let leaf = s.rsplit("::").next().unwrap_or(s);
        hir.is_ffi_struct_class(leaf)
    });
    let is_ffi_struct = is_ffi_union
        || inherits_struct
        || matches!(
            superclass,
            Some(
                "FFI::Struct"
                    | "::FFI::Struct"
                    | "FFI::ManagedStruct"
                    | "::FFI::ManagedStruct"
                    | "FFI::Struct::ManagedStruct"
            )
        );
    // Recorded even when no `layout` follows (an EMPTY body, or ffi_dry's
    // `dsl_layout` building one at runtime): a SIGNATURE naming this class
    // only needs the by-reference fact -- so this precedes the empty-body
    // return below. See `FfiVocab::ffi_struct_classes`.
    if is_ffi_struct && let Some(name) = cref {
        let leaf = name.rsplit("::").next().unwrap_or(name);
        hir.mark_ffi_struct_class(leaf);
        // Marked, not given the parent's LAYOUT: ruby-ffi does not inherit one
        // (`class B < A; end` leaves `B.size` at 0 and `B.new` raises "no
        // Struct layout configured", oracle-verified). The mark is the
        // by-reference fact a signature needs, which every struct class has
        // whether or not it declared fields.
    }
    let stmts: Vec<Node<'_>> = match body {
        None => return Ok(Vec::new()),
        Some(n) => match n.as_statements_node() {
            Some(stmts) => stmts.body().iter().collect(),
            None => vec![n],
        },
    };
    let mut out = Vec::new();
    // The DEFAULT visibility for every subsequent `def` in this class body,
    // switched by a bare `private`/`public`/`protected` (no arguments) --
    // see `lower_class_body_statement`'s docs.
    let mut visibility = Visibility::Public;
    let mut module_function = false;
    // A module that `extend FFI::Library` (the real `ffi` gem) turns its
    // `ffi_lib`/`attach_function` directives into synthesized wrapper class
    // methods over `extern "C"` symbols -- see `lower_ffi_directive`. A
    // NON-FFI statement in such a module still lowers normally (a module may
    // mix), so this only re-routes the recognized directives.
    // `extend FFI::Library` marks the module ONCE; a reopening in another file
    // inherits it by path -- see `Hir::mark_ffi_library`.
    let ffi_path = cref.map(|n| hir.cref_path(n));
    // A `def self.extended(host)` hook that extends FFI::Library into its
    // host makes THIS module an FFI-library extender: record it (with its
    // replayable `host.typedef` stream) so an `extend <this module>` in a
    // later body is recognized as the FFI marker one step removed -- chef's
    // Win32 API modules all take that route.
    if let Some(p) = &ffi_path {
        for stmt in &stmts {
            if let Some(pairs) = ffi_extender_hook(stmt) {
                hir.ffi.ffi_extenders.insert(p.clone(), pairs);
            }
            // The struct-side twin: a hook that installs a LAYOUT on whoever
            // includes it. See `FfiVocab::ffi_layout_hooks`.
            if let Some(source) = crate::lower::ffi::ffi_layout_hook(result, stmt) {
                hir.ffi.ffi_layout_hooks.insert(p.clone(), source);
            }
        }
    }
    // A SNIPPET declares no FFI library: the directives are compile-time
    // ones the whole-program walk consumes, and a snippet has no such
    // walk. Left as ordinary calls, they reach `FFI::Library`'s own rows,
    // which say so loudly instead of silently attaching nothing.
    let is_ffi = !hir.mode.is_eval()
        && (stmts.iter().any(is_extend_ffi_library)
            || ffi_path.as_deref().is_some_and(|p| hir.is_ffi_library(p))
            || stmts.iter().any(|s| ffi_extender_pairs(hir, s).is_some()));
    if is_ffi && let Some(p) = &ffi_path {
        hir.mark_ffi_library(p);
    }
    // `extend FFI::DataConverter` + `native_type T`: the class stands for T
    // in every later type position, keyed by its leaf name like the rest of
    // the FFI type table.
    if stmts
        .iter()
        .any(crate::lower::ffi::is_extend_ffi_data_converter)
        && let Some(ty) = stmts
            .iter()
            .find_map(|s| crate::lower::ffi::native_type_of(s))
        && let Some(name) = cref
    {
        let leaf = name.rsplit("::").next().unwrap_or(name);
        hir.declare_ffi_type(leaf, &ty);
    }
    let mut ffi_lib = crate::hir::FfiLib::None;
    // `typedef :existing, :alias` names accumulated in source order, so a later
    // `attach_function` can name an alias the gem requires be declared first.
    // Seeded with what enclosing/earlier FFI libraries declared, so a struct
    // nested in a library module can name that module's `enum`/`typedef`
    // types -- see `FfiVocab::ffi_types`. Bodies lower in source order, so the
    // declaration is already recorded by the time the nested body starts.
    let mut ffi_aliases: crate::compiler::FMap<String, crate::hir::FfiType> =
        hir.inherited_ffi_types();
    // Replay each extender's recorded `host.typedef` stream, in its source
    // order, before any of this body's own directives lower. A source type
    // that doesn't resolve is skipped -- the alias it would have made stays
    // undeclared, and a later use of it is an honest rejection at that site.
    for stmt in &stmts {
        let Some(pairs) = ffi_extender_pairs(hir, stmt) else {
            continue;
        };
        for (src, alias) in pairs {
            if let Ok(ty) = crate::lower::ffi::ffi_type_of(&src, &ffi_aliases) {
                hir.declare_ffi_type(&alias, &ty);
                ffi_aliases.insert(alias, ty);
            }
        }
    }
    // This is the ONE place a `class`/`module` body's statements are lowered
    // (the runtime-class desugars route through here too), so it is also the
    // one place the cref chain deepens -- see `Hir::cvar_is_toplevel`.
    let mut lower_stmts = |hir: &mut Hir| {
        let mut st = LowerBodyStmt {
            is_ffi,
            is_ffi_struct,
            is_ffi_union,
            cref,
            ffi_lib: &mut ffi_lib,
            ffi_aliases: &mut ffi_aliases,
            visibility: &mut visibility,
            module_function: &mut module_function,
            body: &stmts,
        };
        for stmt in &stmts {
            // Located per STATEMENT, around the whole dispatch below -- an
            // `ffi_lib` or a `layout` never reaches `lower_class_body_statement`
            // (nor `lower_node`), so without a frame here the innermost live one
            // is the enclosing `class`/`module` header and every rejection names
            // that line instead of its own.
            let span = crate::lower::span_of(hir, stmt);
            hir.push_span(span);
            let done = lower_one_class_body_stmt(result, hir, stmt, &mut st, &mut out);
            hir.pop_span();
            done.map_err(|e| e.with_span_if_missing(span))?;
        }
        PResult::Ok(())
    };
    match cref {
        Some(name) => hir.in_class_body(name, lower_stmts)?,
        None => lower_stmts(hir)?,
    }
    Ok(out)
}

/// `attr_reader :a, :b` -> a `DefMethod` getter per name (`body: [IvarRead]`).
/// `attr_writer :a, :b` -> a `DefMethod` setter per name (`name=`, one
/// required param, `body: [IvarWrite]`). `attr_accessor` emits both. Only
/// literal symbol arguments are recognized (matching `define_method`'s own
/// literal-name restriction elsewhere in this file); anything else falls
/// through to an ordinary `Call` (which real Ruby would resolve dynamically,
/// e.g. `attr_reader(*names)` -- unsupported, a clean rejection at
/// codegen if `attr_reader` itself isn't otherwise defined). Every
/// synthesized getter/setter gets the CURRENT default `visibility`, exactly
/// like an ordinary `def` would.
///
/// `private`/`public`/`protected` recognize three real Ruby forms, appending
/// nothing to `out` themselves (they're never a standalone HIR node): (1) a
/// bare call with no arguments switches the DEFAULT `visibility` for every
/// `def` for the REST of this class body; (2) `private def name; ... end`
/// (the `def`-as-sole-argument idiom) lowers the `def` normally through the
/// generic `lower_node` path, then retroactively overrides ITS OWN
/// visibility; (3) `private :name1, :name2, ...` retroactively overrides
/// the visibility of already-lowered method(s) of those names (searched in
/// `out`, everything lowered so far in this same class body -- real Ruby
/// requires the target already be defined earlier in the same body, so no
/// forward search is needed). Anything else (a dynamic/computed argument)
/// falls through to an ordinary `Call` -- a clean rejection at codegen time
/// if `private`/`public`/`protected` themselves aren't otherwise defined,
/// matching this function's own posture elsewhere.
/// The per-body state `lower_one_class_body_stmt` threads through -- bundled
/// so the dispatch keeps one argument per thing rather than eight.
struct LowerBodyStmt<'a> {
    is_ffi: bool,
    is_ffi_struct: bool,
    is_ffi_union: bool,
    /// The enclosing class's name as written -- what a recorded struct
    /// layout is keyed and reported by.
    cref: Option<&'a str>,
    ffi_lib: &'a mut crate::hir::FfiLib,
    ffi_aliases: &'a mut crate::compiler::FMap<String, crate::hir::FfiType>,
    visibility: &'a mut Visibility,
    module_function: &'a mut bool,
    /// The whole class body as written. An FFI directive spelled over a
    /// body-local array -- `layout(*members)` after a run of guarded
    /// `members.push` calls -- replays the statements that precede it to read
    /// the array back; see `ffi::splat_local_elements`.
    body: &'a [Node<'a>],
}

/// The recorded typedef stream of the FFI-library extender an `extend X`
/// statement names, or `None` when the statement is anything else. The
/// extender was recorded under its FULL cref path; the extend site may spell
/// a shorter relative path, so a trailing-components match answers too
/// (`extend Win32::API` finds `Chef::ReservedNames::Win32::API`).
fn ffi_extender_pairs(hir: &Hir, stmt: &Node<'_>) -> Option<Vec<(String, String)>> {
    let written = extend_target_path(stmt)?;
    let written = written.trim_start_matches("::");
    hir.ffi.ffi_extenders.iter().find_map(|(recorded, pairs)| {
        (recorded == written || recorded.ends_with(&format!("::{written}"))).then(|| pairs.clone())
    })
}

/// The three things a class-body statement can be: an FFI directive, an FFI
/// `layout`, or an ordinary statement. Split out of `lower_class_body`'s loop
/// so the loop can wrap ALL of them in one span frame.
fn lower_one_class_body_stmt<'a>(
    result: &ParseResult,
    hir: &mut Hir,
    stmt: &Node<'a>,
    st: &mut LowerBodyStmt<'a>,
    out: &mut Vec<NodeId>,
) -> PResult<()> {
    // A class-body `if`/`unless` with a statically-decided predicate is folded
    // at definition time -- real Ruby runs these guards while the class body
    // executes, and a `def`/`alias`/FFI directive inside one has no ordinary
    // value-`if` lowering. Checked HERE rather than in
    // `lower_class_body_statement` so a directive under a platform gate
    // re-enters the FULL dispatch (FFI included). A dynamic predicate falls
    // through to the generic value-`if` path unchanged.
    if let Some(if_node) = stmt.as_if_node()
        && let Some(cond) = static_guard(&if_node.predicate())
    {
        let chosen = if cond {
            if_node.statements().map(|s| s.as_node())
        } else {
            if_node.subsequent()
        };
        return lower_class_body_selected(result, hir, stmt, chosen, st, out);
    }
    if let Some(unless_node) = stmt.as_unless_node()
        && let Some(cond) = static_guard(&unless_node.predicate())
    {
        let chosen = if !cond {
            unless_node.statements().map(|s| s.as_node())
        } else {
            unless_node.else_clause().map(|e| e.as_node())
        };
        return lower_class_body_selected(result, hir, stmt, chosen, st, out);
    }
    // `case RbConfig::CONFIG['host_os'] when /linux/i then BUFSIZE = 65 ...` --
    // the same platform question written as a case. Folded for the same
    // reason: the branches define ONE constant three times, and a `case` left
    // to run at runtime hands the FFI vocabulary three conflicting answers.
    if let Some(case_node) = stmt.as_case_node()
        && let Some(chosen) = static_case_branch(&case_node)
    {
        return lower_class_body_selected(result, hir, stmt, chosen, st, out);
    }
    // A class-body `CONST = :symbol` / `CONST = <int>` feeds the FFI
    // vocabulary side maps (poison-on-conflict; see `FfiVocab::ffi_symbol_consts`)
    // and STILL lowers normally below -- a nested struct's `layout` resolves
    // its enclosing module's spelling through them.
    if let Some(write) = stmt.as_constant_write_node() {
        let name = String::from_utf8_lossy(write.name().as_slice()).into_owned();
        if let Some(sym) = write.value().as_symbol_node() {
            let val = String::from_utf8_lossy(sym.unescaped()).into_owned();
            match hir.ffi.ffi_symbol_consts.get(&name) {
                Some(Some(prev)) if *prev != val => {
                    hir.ffi.ffi_symbol_consts.insert(name.clone(), None);
                }
                Some(None) => {}
                _ => {
                    hir.ffi
                        .ffi_symbol_consts
                        .insert(name.clone(), Some(val.clone()));
                }
            }
            // A symbol that spells a TYPE joins the declared vocabulary too,
            // so a SIGNATURE naming the constant resolves (`Word = :uint32;
            // attach_function :f, [Word], :void` -- smartcard). ruby-ffi's
            // own `find_type` resolves the constant's value the same way.
            if (st.is_ffi || st.is_ffi_struct || st.is_ffi_union)
                && let Ok(ty) = crate::lower::ffi::ffi_type_of(&val, st.ffi_aliases)
            {
                hir.declare_ffi_type(&name, &ty);
                st.ffi_aliases.insert(name.clone(), ty);
            }
        } else if (st.is_ffi || st.is_ffi_struct || st.is_ffi_union)
            && let Some(ty) = crate::lower::ffi::const_path_string(&write.value())
                .and_then(|p| crate::lower::ffi::ffi_type_constant_of(&p))
        {
            // `CFIndex = FFI::Type::LONG_LONG` (audio's CoreFoundation
            // vocabulary) -- the FFI::Type constant IS the type.
            hir.declare_ffi_type(&name, &ty);
            st.ffi_aliases.insert(name, ty);
        } else if let Some(val) = crate::lower::ffi::ffi_const_int(&write.value(), hir, out) {
            // The same folder an enum member's value goes through, not a bare
            // integer literal: `RTMP_BUFFER_CACHE_SIZE = (16*1024)` is an
            // inline array's element COUNT four lines below, and a count is
            // what decides where every following field starts.
            match hir.ffi.ffi_int_consts.get(&name) {
                Some(Some(prev)) if *prev != val => {
                    hir.ffi.ffi_int_consts.insert(name, None);
                }
                Some(None) => {}
                _ => {
                    hir.ffi.ffi_int_consts.insert(name, Some(val));
                }
            }
        }
    }
    // `FFI.typedef :existing, :alias` -- the GLOBAL registry the gem keeps on
    // the FFI module itself, visible to every library and struct that lowers
    // after it (puppet fills it with the Win32 vocabulary in one file and
    // spends it across the rest). Not gated on `is_ffi`: the enclosing module
    // is usually a plain namespace.
    if let Some((existing, alias)) = as_global_ffi_typedef(stmt, st.ffi_aliases, st.cref) {
        hir.declare_ffi_type(&alias, &existing);
        st.ffi_aliases.insert(alias, existing);
        return Ok(());
    }
    if st.is_ffi {
        // `extend FFI::Library` is the marker AND an ordinary `extend`: the
        // directives below it are consumed at compile time, but the module
        // really does extend `FFI::Library` in ruby -- which is what makes
        // `M.is_a?(FFI::Library)` true and puts the run-time tier's own
        // `typedef`/`attach_variable` rows in reach. So it falls through.
        if lower_ffi_directive(result, hir, stmt, st.ffi_lib, st.ffi_aliases, out, st.body)? {
            return Ok(());
        }
    }
    // `include <a module whose self.included hook class_evals a layout>` --
    // the layout, and every `def` beside it, belong to THIS struct. Replayed
    // from the hook's recorded source (see `FfiVocab::ffi_layout_hooks`): the
    // statements then take the ordinary struct-body path below, so the layout
    // synthesizes accessors and records offsets exactly as a written one does.
    if st.is_ffi_struct
        && let Some(source) = ffi_layout_hook_source(hir, stmt)
    {
        // LEAKED, both of them. `lower_one_class_body_stmt` ties the statement
        // it lowers to the same lifetime as the class body it lowers against
        // (`LowerBodyStmt<'a>`), and these statements come from a different
        // parse entirely -- so they have to outlive it. The cost is the hook's
        // source and its `ParseResult` per include site: tens of bytes, a
        // handful of sites in the one gem family that writes this, and the
        // compiler is a one-shot process.
        let source: &'static str = Box::leak(source.into_boxed_str());
        let replayed: &'static ruby_prism::ParseResult<'static> =
            Box::leak(Box::new(ruby_prism::parse(source.as_bytes())));
        if let Some(err) = replayed.errors().next() {
            return Err(format!("replayed layout hook: parse error: {}", err.message()).into());
        }
        let program = replayed
            .node()
            .as_program_node()
            .ok_or("expected a top-level ProgramNode")?;
        let stmts: Vec<Node<'static>> = program.statements().body().iter().collect();
        // `st.body` stays as it was: a body-local replay inside the hook
        // resolves against the STRUCT's real class body, not the snippet.
        for s in &stmts {
            lower_one_class_body_stmt(replayed, hir, s, st, out)?;
        }
        return Ok(());
    }
    if st.is_ffi_struct
        && let Some(fields) = as_ffi_layout(stmt, st.ffi_aliases, hir, out, st.body)?
    {
        // Replace `layout ...` in place with the synthesized accessors, so any
        // user methods after it can still override them. The inline-array proxy
        // classes ride along with the FIRST struct that needs them -- see
        // `claim_ffi_inline_array_classes`.
        let classes = crate::lower::ffi::needs_inline_array_classes(&fields)
            && hir.claim_ffi_inline_array_classes();
        // ONE layout walk: the accessor synthesis reads the same offsets that
        // get RECORDED (keyed by leaf name like the rest of the FFI type
        // table) for a later `attach_function` to pass this struct by value.
        let layout =
            crate::lower::ffi::ffi_struct_layout(st.cref.unwrap_or(""), &fields, st.is_ffi_union)?;
        let source = synthesize_ffi_struct(&layout, classes)?;
        if let Some(name) = st.cref {
            let leaf = name.rsplit("::").next().unwrap_or(name);
            hir.ffi.ffi_struct_layouts.insert(leaf.to_string(), layout);
        }
        // In a SNIPPET the accessors are never emitted: a class body inside
        // an `eval` runs as one more `class_eval` of its own source text,
        // which the emitter recovers from the statements' SPANS. So the
        // synthesized text has to be a file of its own there, or the spans
        // would be offsets into it read against the snippet. In a whole
        // program the nodes are what runs and the enclosing file is the
        // right provenance -- a synthesized accessor reports the line its
        // `layout` was written on.
        out.extend(if hir.mode.is_eval() {
            let name = format!("<ffi-struct:{}>", st.cref.unwrap_or("?"));
            let file = hir.add_file(name, source.as_str());
            let prev = hir.lowering_file.replace(file);
            let ids = parse_and_lower_into(hir, &source);
            hir.lowering_file = prev;
            ids?
        } else {
            parse_and_lower_into(hir, &source)?
        });
        return Ok(());
    }
    lower_class_body_statement(result, hir, stmt, st.visibility, st.module_function, out)
}

/// The recorded hook source an `include M` inside a struct body replays, or
/// `None` when this is an ordinary mixin.
///
/// The module is looked up the way the include site would resolve it --
/// innermost lexical scope first, then outward -- which is what lets
/// `include GssBufferDescLayout` inside `GSSAPI::LibGSSAPI::UnManaged…` find
/// `GSSAPI::LibGSSAPI::GssBufferDescLayout`.
fn ffi_layout_hook_source(hir: &Hir, stmt: &Node<'_>) -> Option<String> {
    let call = stmt.as_call_node()?;
    if call.receiver().is_some() || call.name().as_slice() != b"include" {
        return None;
    }
    let args: Vec<Node<'_>> = call.arguments()?.arguments().iter().collect();
    let [only] = args.as_slice() else {
        return None;
    };
    let path = crate::lower::ffi::const_path_string(only)?;
    hir.ffi_layout_hook_for(&path).cloned()
}

fn lower_class_body_statement(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    visibility: &mut Visibility,
    module_function: &mut bool,
    out: &mut Vec<NodeId>,
) -> PResult<()> {
    // `alias new_name old_name` / `alias :new_name :old_name` (`AliasMethodNode`
    // -- a real Ruby KEYWORD, not a method call, so this is checked before the
    // `as_call_node()` cascade below). `old_name` must already be defined
    // EARLIER in this SAME class/module body (searched in `out`, exactly the
    // same "no forward search, no ancestor walk" restriction `private
    // :name1, :name2` already enforces above) -- aliasing an INHERITED
    // method is a clean rejection, a documented, narrow scope-cut. Resolved
    // entirely at LOWERING time: since the found `DefMethod`'s `params`/
    // `body`/`is_class_method`/`visibility` are all cheaply `Clone`-able,
    // the alias is just a second `DefMethod` node under a different name --
    // no new analyze-phase machinery, no shared-body indirection to keep in
    // sync with `super`/materialization.
    // `undef foo, bar` -- a keyword like `alias`, same target shape (prism
    // gives each name as a SymbolNode either way), so it reuses
    // `alias_target_name`. Recorded rather than resolved here: see
    // `HirNode::Undef` for why the inherited case rules out deleting a def.
    if let Some(undef) = node.as_undef_node() {
        // An INTERPOLATED name has no compile-time spelling to record on
        // `HirNode::Undef`, but the keyword is still a runtime tombstone on
        // the default definee -- the same `undef_method` send the
        // expression-position arm desugars to, as a body statement executing
        // in class-body order. All of the statement's names ride the send so
        // they still undefine in written order.
        if undef
            .names()
            .iter()
            .any(|n| n.as_interpolated_symbol_node().is_some())
        {
            let args = undef
                .names()
                .iter()
                .map(|n| match n.as_interpolated_symbol_node() {
                    Some(_) => Ok(ArrayElem::Single(lower_node(result, hir, &n)?)),
                    None => {
                        let name = alias_target_name(&n)?;
                        Ok(ArrayElem::Single(hir.push(HirNode::SymbolLit(name))))
                    }
                })
                .collect::<PResult<Vec<_>>>()?;
            out.push(hir.push(HirNode::Call {
                receiver: None,
                name: "undef_method".to_string(),
                args,
                kwargs: Vec::new(),
                block: None,
                block_arg: None,
                safe: false,
            }));
            return Ok(());
        }
        let names = undef
            .names()
            .iter()
            .map(|n| alias_target_name(&n))
            .collect::<PResult<Vec<_>>>()?;
        out.push(hir.push(HirNode::Undef(names)));
        return Ok(());
    }

    if let Some(alias) = node.as_alias_method_node() {
        // An INTERPOLATED name (`alias :"#{kind}_attr" :"#{kind}_attrs"`,
        // formal_wear building its DSL in a loop) has no compile-time
        // spelling to register, but the `alias` keyword is still just a
        // runtime install on the default definee -- the same
        // `__zeo_alias_keyword` desugar the general-context arm uses, as a
        // body statement executing in class-body order. Call sites for a
        // computed name have no static row to bind, so they reach the
        // runtime alias through the dynamic fallback on their own.
        let interpolated = alias.new_name().as_interpolated_symbol_node().is_some()
            || alias.old_name().as_interpolated_symbol_node().is_some();
        if interpolated {
            let new_id = lower_node(result, hir, &alias.new_name())?;
            let old_id = lower_node(result, hir, &alias.old_name())?;
            let send = hir.push(HirNode::Call {
                receiver: None,
                name: "__zeo_alias_keyword".to_string(),
                args: vec![ArrayElem::Single(new_id), ArrayElem::Single(old_id)],
                kwargs: Vec::new(),
                block: None,
                block_arg: None,
                safe: false,
            });
            out.push(send);
            return Ok(());
        }
        let new_name = alias_target_name(&alias.new_name())?;
        let old_name = alias_target_name(&alias.old_name())?;
        push_alias(hir, out, new_name, old_name);
        return Ok(());
    }

    // `class << self ... end` (`SingletonClassNode`) -- reopens the class's
    // OWN singleton class, the idiomatic way to define several class
    // methods at once without repeating `def self.` on each one. `class <<
    // obj` on any expression OTHER than a bare `self` is a per-instance
    // singleton class -- a materially bigger feature (a dynamically-
    // growable per-instance vtable) zeo doesn't support, matching
    // the plan's existing scope-cut on `define_singleton_method`; a clean
    // rejection, not silently ignored. The nested body is lowered through
    // the ORDINARY class-body path (so `attr_reader`/`private`/`alias`/
    // nested `def`s all work exactly as they would directly in the class
    // body), then each result is mapped onto the ENCLOSING class:
    //   - a `def`     -> retagged as a class method (`set_method_is_class_method`);
    //   - a constant, and a nested `class`/`module` -> wrapped in the
    //     SINGLETON SURROGATE at its own position (`homes_on_the_singleton`),
    //     which is where ruby files both: `C::NAME` raises and
    //     `C.singleton_class.const_defined?(:NAME)` is true. The singleton's
    //     own methods still read it by bare name, the surrogate being their
    //     lexical home;
    //   - `include M` -> `extend M` on the enclosing class (M's instance
    //     methods become class methods either way -- same effect).
    //   - `prepend M`/`undef`/`private :m` -> their class-method halves
    //     (`ClassMethodPrepend`/`ClassMethodUndef`/`ClassMethodVisibility`);
    //   - any other call, and `extend M` -> rebound onto
    //     `self.singleton_class`, the receiver real Ruby runs them against.
    // A nested `class << self` re-enters this same arm one level deeper.
    if let Some(singleton) = node.as_singleton_class_node() {
        // `class << HTTP` written INSIDE `class HTTP` IS `class << self` --
        // net/http spells its class-method aliases that way, and routing it
        // through the per-object desugar would install them on a runtime
        // singleton the compile-time tables never see. Same rule (and same
        // reason) as `def HTTP.version_1_2` -- see `lower::names_enclosing_class`.
        let is_self = singleton.expression().as_self_node().is_some()
            || crate::lower::names_enclosing_class(hir, &singleton.expression());
        if !is_self {
            // `class << obj` on a NON-`self` receiver: each `def` in
            // the body is a per-object singleton method (see
            // `desugar_singleton_class_defs`).
            out.extend(desugar_singleton_class_defs(result, hir, &singleton)?);
            return Ok(());
        }
        // A `class << self` among the statements of ANOTHER `class << self`
        // body opens the surrogate's own singleton -- the same construct one
        // level deeper, so it takes the same route one level deeper. Its
        // mapped items become the body of a reopen of the surrogate, where
        // they mean on `Foo.singleton_class` exactly what they would mean on
        // `Foo` written directly in its class body: a `def` retagged as a
        // class method of the surrogate IS an instance method of
        // `Foo.singleton_class.singleton_class`, which is where ruby puts it.
        // lita's `class << self; class << self; def define_deprecated_class_method`
        // is the shape, and the `define_deprecated_class_method :add_user_to_group`
        // calls beside it -- rebound onto `self.singleton_class`, i.e. the
        // surrogate -- then find it.
        let nested = hir.is_in_singleton_body();
        let mut inner = hir
            .in_singleton_body(|hir| lower_class_body(result, hir, singleton.body(), None, None))?;
        // A bare visibility directive survived as a marker (see the
        // `private` arm below): its runtime default on the surrogate must
        // die with THIS body, as CRuby's cursor dies with the cref --
        // append a reset for the mapping to rebind alongside the markers.
        let bare_vis = |hir: &Hir, n: NodeId| {
            matches!(
                &hir[n],
                HirNode::Call { receiver: None, name, args, kwargs, block, block_arg, .. }
                    if args.is_empty()
                        && kwargs.is_empty()
                        && block.is_none()
                        && block_arg.is_none()
                        && matches!(name.as_str(), "private" | "public" | "protected")
            )
        };
        if inner.iter().any(|&n| bare_vis(hir, n)) {
            // The `class << self` keyword's own span, like every other node
            // synthesized here. A span-less statement in a class body cannot
            // be re-emitted as SOURCE TEXT, which is how a body written inside
            // a run-time `eval` runs (`clif::eval::eval_body_source`), so a
            // `class << self` holding a bare `private` refused the whole
            // compile -- rubygems' `platform.rb` is the corpus case.
            hir.push_span(crate::lower::span_of(hir, node));
            let reset = hir.push(HirNode::Call {
                receiver: None,
                name: "public".to_string(),
                args: Vec::new(),
                kwargs: Vec::new(),
                block: None,
                block_arg: None,
                safe: false,
            });
            hir.pop_span();
            inner.push(reset);
        }
        let mut mapped = Vec::new();
        map_class_self_items(hir, &inner, &mut mapped)?;
        if nested {
            // The wrapper reopen merges with the ENCLOSING singleton body's
            // surrogate (same reserved name, same lexical parent once the
            // outer mapping hoists it), which is where the retagged `def`s
            // belong: a nested singleton body's method IS a class method of
            // the outer surrogate. Each CONSTANT wraps ONE level deeper --
            // the inner reopen registers with the wrapper itself as lexical
            // parent, minting the surrogate's OWN singleton, where ruby
            // homes it (`K.singleton_class.singleton_class`, not
            // `K.singleton_class`). The `def`s beside it are tagged so
            // their bare reads resolve against that inner class
            // (`Scope::lexical_home` finds it by lexical parent).
            let mut body_items = Vec::with_capacity(mapped.len());
            let mut rest = Vec::new();
            for n in mapped {
                if !homes_on_the_singleton(hir, n) {
                    rest.push(n);
                    body_items.push(n);
                    continue;
                }
                let cspan = hir.span(n).unwrap_or(crate::hir::Span::SYNTH);
                hir.push_span(cspan);
                let inner = hir.push(HirNode::ClassDef {
                    name: SINGLETON_SURROGATE.to_string(),
                    superclass: None,
                    body: vec![n],
                    is_module: false,
                });
                hir.pop_span();
                body_items.push(inner);
            }
            tag_singleton_body_defs(hir, &rest);
            // The reopen carries the nested `class << self`'s own location: a
            // class body takes its backtrace frame from its definition node,
            // and a span-less one is emitted with no frame at all.
            let span = crate::lower::span_of(hir, node);
            hir.push_span(span);
            let def = hir.push(HirNode::ClassDef {
                name: SINGLETON_SURROGATE.to_string(),
                superclass: None,
                body: body_items,
                is_module: false,
            });
            hir.pop_span();
            out.push(def);
            return Ok(());
        }
        // A constant assigned here belongs to the SINGLETON class, not the
        // enclosing module (`M.const_defined?(:SC)` is false where
        // `M.singleton_class.const_defined?(:SC)` is true). The constants
        // move into a surrogate child definition under the reserved name
        // `#<Class:self>` -- no Ruby constant can collide with it -- which
        // `analyze::register_class` files as an ordinary module and the
        // runtime singleton mint answers for `M.singleton_class` (see
        // `zeo_rt::register_singleton_surrogate`). The `def`s beside them
        // are tagged: their lexical home is the singleton, so a bare `SC`
        // resolves against the surrogate and `Module.nesting` reports it.
        // One reopen per constant, spliced in AT ITS POSITION rather than
        // collected into one body at the front: the constant's VALUE is an
        // expression that the statements before it can decide (`$n = 5; V =
        // $n`), so hoisting it evaluated it too early and answered the value
        // from before the body ran. Same rule, and the same reason, as the
        // `SingletonBody` arm above.
        let mut minted = false;
        let mut rest = Vec::with_capacity(mapped.len());
        let mut ordered = Vec::with_capacity(mapped.len());
        for n in mapped {
            if matches!(&hir[n], HirNode::ClassDef { name, .. } if name == SINGLETON_SURROGATE) {
                // A residual statement's own reopen (the `SingletonBody` arm
                // above) already IS a surrogate mint.
                minted = true;
                ordered.push(n);
                continue;
            }
            if !homes_on_the_singleton(hir, n) {
                rest.push(n);
                ordered.push(n);
                continue;
            }
            minted = true;
            let span = hir.span(n).unwrap_or(crate::hir::Span::SYNTH);
            hir.push_span(span);
            let def = hir.push(HirNode::ClassDef {
                name: SINGLETON_SURROGATE.to_string(),
                superclass: None,
                body: vec![n],
                is_module: false,
            });
            hir.pop_span();
            ordered.push(def);
        }
        // EVERY `class << self` body mints the surrogate, not just a
        // constant-bearing one: the surrogate IS the body's cref, so a `def`
        // in a block inside a def here must define on the SINGLETON (a class
        // method of the enclosing class), and `Module.nesting` reports it.
        // A body that minted nothing above gets one empty reopen up front.
        if !minted {
            let span = crate::lower::span_of(hir, node);
            hir.push_span(span);
            let def = hir.push(HirNode::ClassDef {
                name: SINGLETON_SURROGATE.to_string(),
                superclass: None,
                body: Vec::new(),
                is_module: false,
            });
            hir.pop_span();
            ordered.insert(0, def);
        }
        tag_singleton_body_defs(hir, &rest);
        // Ruby gives the whole `class << self` body a backtrace frame of its
        // own, labelled `singleton class`. These statements are spliced into
        // the ENCLOSING class body, so codegen has to put the frame back
        // around them (`emit_body`'s grouping). The surrogate reopens are
        // excluded: a class body already pushes a frame, and `body_frame_
        // label` spells the surrogate's the same way, so framing one twice
        // would report the singleton frame twice.
        //
        // The anchor carries the `class << self` keyword's own span, which is
        // where the frame's line and `end` line come from. It is never
        // emitted; it exists so the readers stay `source_location` /
        // `source_end_line`, the pair every other frame uses.
        hir.push_span(crate::lower::span_of(hir, node));
        let self_node = hir.push(HirNode::NilLit);
        hir.pop_span();
        for &n in &ordered {
            if matches!(&hir[n], HirNode::ClassDef { name, .. } if name == SINGLETON_SURROGATE) {
                continue;
            }
            hir.singleton_frame_stmts.insert(n, self_node);
        }
        out.extend(ordered);
        return Ok(());
    }

    // The receiverless class-body DIRECTIVES -- see `directives.rs` for the
    // nine handlers. The arms are name-disjoint, so their order here is NOT
    // load-bearing (unlike `calls::lower_call_node`'s chain). Every handler
    // answers `false` for a dynamic/non-literal shape, which falls through
    // to the generic statement lowering below -- the runtime `Call` those
    // shapes always took. The `visibility`/`module_function` cursors a
    // handler mutates feed every LATER statement of this same body.
    if let Some(call) = node.as_call_node()
        && call.receiver().is_none()
    {
        let name = String::from_utf8_lossy(call.name().as_slice()).into_owned();
        let handled = match name.as_str() {
            "private" | "public" | "protected" => {
                directives::visibility(result, hir, node, &call, &name, visibility, out)?
            }
            "private_class_method" | "public_class_method" => directives::class_method_visibility(
                result,
                hir,
                &call,
                &name,
                visibility,
                module_function,
                out,
            )?,
            "private_constant" | "public_constant" => {
                directives::constant_visibility(hir, &call, &name, out)?
            }
            "module_function" => {
                directives::module_function(hir, &call, &name, module_function, out)?
            }
            "alias_method" => directives::alias_method(hir, &call, &name, out)?,
            "include" | "extend" | "prepend" => directives::mixin(hir, node, &call, &name, out)?,
            "refine" => directives::refine(result, hir, node, &call, &name, out)?,
            "using" => directives::using(result, hir, node, &name, &call, out)?,
            "attr" | "attr_reader" | "attr_writer" | "attr_accessor" => {
                directives::attr(hir, node, &call, &name, *visibility, out)?
            }
            _ => false,
        };
        if handled {
            return Ok(());
        }
    }
    // An ordinary `def` gets the CURRENT default visibility -- the generic
    // `lower_node` path (reached below) always sets `Public` (it has no
    // notion of a class body's running default; see its own docs), so this
    // corrects it retroactively when the current default isn't `Public`.
    if let Some(def) = node.as_def_node() {
        let id = lower_node(result, hir, node)?;
        // The running visibility default and `module_function` promotion apply
        // only to a bare `def name` (an instance method). A SINGLETON def
        // (`def self.name` / `def Recv.name`) defines a method on another
        // object, is unaffected by either in real Ruby, and doesn't lower to a
        // plain instance `DefMethod` -- so leave it exactly as lowered.
        if def.receiver().is_some() {
            out.push(id);
            return Ok(());
        }
        if *visibility != Visibility::Public || auto_private_def(hir, id) {
            let vis = match auto_private_def(hir, id) {
                true => Visibility::Private,
                false => *visibility,
            };
            hir.set_method_visibility(id, vis);
        }
        // Under a bare `module_function`, every following `def` becomes a
        // MODULE method AND a PRIVATE instance method (real Ruby keeps both,
        // so an `include`d module's method is callable bare). The original
        // `def` stays as the private instance method; a class-method copy is
        // added alongside it.
        if *module_function && promote_to_module_function(hir, id, out) {
            return Ok(());
        }
        out.push(id);
        return Ok(());
    }
    // A `def` nested in a RUNTIME-undecidable `if`/`case` branch still runs
    // under the body's running visibility default and `module_function` mode
    // -- CRuby applies both when the branch executes. The statically-foldable
    // guard was peeled above; here the guard stays, so the promotion rewrites
    // the lowered branches IN PLACE: under `module_function` the def turns
    // private and its module-method twin joins it inside the same branch,
    // making the twin exactly as conditional as the def it copies
    // (rspec-support's `RubyFeatures.ripper_supported?` is the corpus case).
    let id = lower_node(result, hir, node)?;
    // `define_method(:x) { ... }` is a CallNode, so it never reached the `def`
    // arm above and kept the generic path's `Public` -- but ruby applies the
    // running default AND `module_function` to it exactly as to a `def`
    // (oracle-checked on 4.0.6: `private` makes it private, and
    // `module_function` gives it both a module method and a private instance
    // copy). It lowers to a `DefMethod` carrying `BLOCK_BODIED_DEF`, which is
    // what tells it apart from the branch containers handled below.
    if hir.has_flag(id, crate::hir::NodeFlag::BLOCK_BODIED_DEF)
        && matches!(
            &hir[id],
            crate::hir::HirNode::DefMethod {
                is_class_method: false,
                ..
            }
        )
    {
        if *visibility != Visibility::Public || auto_private_def(hir, id) {
            let vis = match auto_private_def(hir, id) {
                true => Visibility::Private,
                false => *visibility,
            };
            hir.set_method_visibility(id, vis);
        }
        if *module_function && promote_to_module_function(hir, id, out) {
            return Ok(());
        }
        out.push(id);
        return Ok(());
    }
    if *module_function || *visibility != Visibility::Public {
        apply_body_defaults_in_branches(hir, id, *visibility, *module_function);
    }
    push_body_statement(hir, id, *module_function, out);
    Ok(())
}

/// Whether a `class << self` statement names something ruby files on the
/// SINGLETON class rather than on the enclosing one -- a constant, or a
/// nested `class`/`module`. Each such statement is wrapped in its own
/// surrogate reopen, spliced at its own position.
///
/// A nested class is the same fact as a constant, because a `class X` IS a
/// constant write with a body: `class << self; class Visitor; end; end`
/// leaves `M.singleton_class.const_defined?(:Visitor)` true and `M::Visitor`
/// raising. The two used to be classified apart, so the class landed on the
/// enclosing module and answered a lookup ruby refuses.
///
/// A surrogate is NOT one of these. It is the wrapper itself -- a residual
/// statement's own mint, or a nested `class << self`'s reopen -- and
/// wrapping it again would file a three-deep singleton body one level too
/// far out.
fn homes_on_the_singleton(hir: &Hir, id: NodeId) -> bool {
    match &hir[id] {
        HirNode::ConstWrite { .. } => true,
        HirNode::ClassDef { name, .. } => name != SINGLETON_SURROGATE,
        _ => false,
    }
}

/// Push one class-body statement, and behind it the promotion a run-time
/// `define_method` needs to obey a bare `module_function`.
///
/// The mode makes every definition after it a module method AND a private
/// instance copy. Zeo resolves that cursor at compile time, which reaches a
/// `def` and reaches `define_method(:name) { }` (it desugars to one) -- but
/// `define_method(:name, <a callable>)` installs at RUN time, where a
/// compile-time cursor cannot follow it. What Ruby produces for that
/// statement is exactly what the runtime's own `Module#module_function(:name)`
/// produces, so emitting that call right behind it carries the mode across
/// the boundary.
fn push_body_statement(hir: &mut Hir, id: NodeId, module_function: bool, out: &mut Vec<NodeId>) {
    out.push(id);
    if !module_function {
        return;
    }
    let Some(name) = runtime_define_method_name(hir, id) else {
        return;
    };
    if let Some(span) = hir.span(id) {
        hir.push_span(span);
    }
    let sym = hir.push(HirNode::SymbolLit(name));
    out.push(hir.push(HirNode::Call {
        receiver: None,
        name: "module_function".to_string(),
        args: vec![crate::hir::ArrayElem::Single(sym)],
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    }));
    if hir.span(id).is_some() {
        hir.pop_span();
    }
}

/// The literal name a `define_method` STATEMENT installs at run time.
///
/// Deliberately blind to what follows the name -- a `Method`, an
/// `UnboundMethod`, a lambda, an `&proc`, or a block that closes over an
/// enclosing local all reach here, and every one of them is a run-time
/// install. The compile-time shapes are `DefMethod` nodes by now and never
/// match.
fn runtime_define_method_name(hir: &Hir, id: NodeId) -> Option<String> {
    let HirNode::Call {
        receiver: None,
        name,
        args,
        ..
    } = &hir[id]
    else {
        return None;
    };
    if name != "define_method" {
        return None;
    }
    let crate::hir::ArrayElem::Single(first) = *args.first()? else {
        return None;
    };
    hir.sent_name(first).map(str::to_string)
}

/// Whether `id` is a `def` ruby makes PRIVATE whatever the running default
/// says. The object-initialization family and `respond_to_missing?` are hooks
/// the runtime calls, never a public API, so `rb_scope_visibility_get` forces
/// their visibility rather than reading it.
fn auto_private_def(hir: &Hir, id: NodeId) -> bool {
    let crate::hir::HirNode::DefMethod {
        name,
        is_class_method: false,
        ..
    } = &hir[id]
    else {
        return false;
    };
    matches!(
        name.as_str(),
        "initialize"
            | "initialize_copy"
            | "initialize_clone"
            | "initialize_dup"
            | "respond_to_missing?"
    )
}

/// Applies the class body's running `visibility` default and `module_function`
/// mode to every instance `def` in `id`'s `if`/`case` branches, recursively --
/// see the call site above for why. Statements other than branch containers
/// and defs pass through untouched.
fn apply_body_defaults_in_branches(
    hir: &mut Hir,
    id: NodeId,
    visibility: Visibility,
    module_function: bool,
) {
    let rewrite = |hir: &mut Hir, stmts: &mut Vec<NodeId>| {
        let mut out = Vec::with_capacity(stmts.len());
        for &sid in stmts.iter() {
            match &hir[sid] {
                HirNode::DefMethod {
                    is_class_method: false,
                    ..
                } => {
                    if module_function {
                        promote_to_module_function(hir, sid, &mut out);
                        continue;
                    }
                    hir.set_method_visibility(sid, visibility);
                    out.push(sid);
                }
                HirNode::If { .. } | HirNode::CaseWhen { .. } => {
                    apply_body_defaults_in_branches(hir, sid, visibility, module_function);
                    out.push(sid);
                }
                // A run-time `define_method` inside a branch needs the same
                // promotion the body's own statements get -- rack writes
                // `define_method(:escape_html, ERB::Escape.instance_method(..))`
                // under an `if defined?(ERB::Escape)`.
                _ => push_body_statement(hir, sid, module_function, &mut out),
            }
        }
        *stmts = out;
    };
    match &hir[id] {
        HirNode::If {
            then_body,
            else_body,
            ..
        } => {
            let (mut t, mut e) = (then_body.clone(), else_body.clone());
            rewrite(hir, &mut t);
            rewrite(hir, &mut e);
            if let HirNode::If {
                then_body,
                else_body,
                ..
            } = &mut hir[id]
            {
                (*then_body, *else_body) = (t, e);
            }
        }
        HirNode::CaseWhen {
            arms, else_body, ..
        } => {
            let mut arms_bodies: Vec<Vec<NodeId>> = arms.iter().map(|(_, b)| b.clone()).collect();
            let mut e = else_body.clone();
            for b in &mut arms_bodies {
                rewrite(hir, b);
            }
            rewrite(hir, &mut e);
            if let HirNode::CaseWhen {
                arms, else_body, ..
            } = &mut hir[id]
            {
                for (arm, b) in arms.iter_mut().zip(arms_bodies) {
                    arm.1 = b;
                }
                *else_body = e;
            }
        }
        _ => {}
    }
}

/// The definition family of [`super::lower_node_inner`]'s recognizer chain:
/// `class` (static, runtime-parent, and runtime-reopen paths), `module`,
/// expression-position `undef`, `def` (instance, `self.`, enclosing-class,
/// and per-object singleton spellings), and `class << obj` outside a class
/// body. `Ok(None)` = not this family's node.
pub(crate) fn try_lower_definition(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<Option<NodeId>> {
    if let Some(class) = node.as_class_node() {
        // `class self::Task` / `class parent::Pagination` -- see
        // `runtime_scoped_definition_name`.
        if let Some((scope_node, leaf)) = runtime_scoped_definition_name(&class.constant_path()) {
            let parent = match class.superclass() {
                Some(sc) => lower_node(result, hir, &sc)?,
                None => hir.push(HirNode::ClassRef("Object".to_string())),
            };
            let scope = lower_node(result, hir, &scope_node)?;
            return lower_scoped_definition(result, hir, scope, leaf, class.body(), Some(parent))
                .map(Some);
        }
        let name = constant_path_name(&class.constant_path())?;
        // A superclass that isn't a constant path (`class Point <
        // Struct.new(:x, :y)`) names a class that only comes into existence at
        // RUN time, so the subclass can't be one of the statically emitted
        // Rust structs -- it has to be minted at runtime too. See
        // `lower_runtime_class`.
        if let Some(sc) = class.superclass() {
            let runtime_parent = match superclass_name(hir, &sc) {
                // Not a constant path at all (`< Struct.new(:x)`).
                Err(_) => true,
                // A constant path that holds a runtime class VALUE (`Base =
                // Class.new` earlier in the file) rather than naming a
                // compile-time one -- the subclass has to be built at runtime
                // for the same reason. A name that is also a `class`
                // definition stays on the static path.
                //
                // Both halves ask FROM THIS CREF, which is what makes them
                // agree: they used to be a lexical search against a bare-leaf
                // table, so a constant of the same leaf in an unrelated
                // namespace answered the first and not the second, and the
                // definition was silently rewritten into a runtime one.
                Ok(n) => const_is_assigned(hir, &n) && !const_is_class_def(hir, &n),
            };
            if runtime_parent {
                return lower_runtime_class(result, hir, &name, Some(&sc), class.body()).map(Some);
            }
        } else if const_holds_runtime_class(hir, &name)
            && !const_is_class_def(hir, &name)
            && runtime_class_body_is_expressible(class.body())
        {
            // No superclass clause, and the name holds a runtime class value
            // (`D = Data.define(:x)`) -- this REOPENS that class rather than
            // defining a new one, so it lowers to a runtime reopen instead of
            // a `ClassDef` the static path would register as a fresh
            // (memberless) class.
            //
            // A body the runtime form can't express falls back to the STATIC
            // path rather than erroring: a constant alias to a builtin
            // (`INT_ALIAS = 1.class; class INT_ALIAS; include M; end`) is a
            // real Ruby shape the static path at least compiles, and turning
            // a program that ran into one that won't build is a worse
            // failure than the one it already had.
            return lower_runtime_class_reopen(result, hir, &name, class.body()).map(Some);
        }
        // `class SecretKeys::Encryptor` where `SecretKeys` is itself a runtime
        // class (`class SecretKeys < DelegateClass(Hash)`, lowered above to a
        // `Class.new` write). The class being defined here is perfectly
        // ordinary -- it is its NAMESPACE that no compile-time class backs, so
        // the static path can only report `unknown class/module SecretKeys`.
        // Minting this one at runtime too puts the constant inside the parent
        // that does exist by then.
        if runtime_scoped_definition(hir, &name) && runtime_class_body_keeps_its_scope(class.body())
        {
            let sc = class.superclass();
            return lower_runtime_class(result, hir, &name, sc.as_ref(), class.body()).map(Some);
        }
        let superclass = match class.superclass() {
            None => None,
            Some(sc) => Some(superclass_name(hir, &sc)?),
        };
        let body = lower_class_body(
            result,
            hir,
            class.body(),
            superclass.as_deref(),
            Some(&name),
        )?;
        hir.record_class_def(&name);
        return Ok(Some(hir.push(HirNode::ClassDef {
            name,
            superclass,
            body,
            is_module: false,
        })));
    }

    // `module Name ... end` -- see `HirNode::ClassDef`'s docs for why this
    // shares the same node as `class`. Nested modules/namespaced constant
    // paths (`module Foo::Bar`) aren't supported yet (zeo limitation, matching
    // today's existing top-level-only class restriction) -- `constant_name`
    // already rejects anything but a plain `ConstantReadNode`.
    if let Some(module) = node.as_module_node() {
        // `module self::Base` -- the module half of the same shape.
        if let Some((scope_node, leaf)) = runtime_scoped_definition_name(&module.constant_path()) {
            let scope = lower_node(result, hir, &scope_node)?;
            return lower_scoped_definition(result, hir, scope, leaf, module.body(), None)
                .map(Some);
        }
        let name = constant_path_name(&module.constant_path())?;
        // The module half of the runtime-scope route above.
        if runtime_scoped_definition(hir, &name)
            && runtime_class_body_keeps_its_scope(module.body())
        {
            return lower_runtime_module(result, hir, &name, module.body()).map(Some);
        }
        let body = lower_class_body(result, hir, module.body(), None, Some(&name))?;
        hir.record_class_def(&name);
        return Ok(Some(hir.push(HirNode::ClassDef {
            name,
            superclass: None,
            body,
            is_module: true,
        })));
    }

    // `undef :a, :b` in EXPRESSION position -- reached when a class-body
    // `undef` sits under a guard zeo can't decide at compile time
    // (`undef :to_a if respond_to?(:to_a)`, drb). `HirNode::Undef` records a
    // compile-time fact and has no value form, so this becomes the runtime
    // send the guard can actually gate: `undef_method` on the class body's
    // `self`, whose overlay tombstone terminates lookup exactly as the static
    // form's does. The unguarded statement form still takes the static path
    // (`lower::defs::lower_class_body_statement`).
    if let Some(undef) = node.as_undef_node() {
        let args = undef
            .names()
            .iter()
            .map(|n| {
                // An INTERPOLATED name (`undef :"#{method}="`,
                // immutable_struct_ex stripping Struct writers in a loop)
                // lowers as the runtime expression it is -- `undef_method`
                // reads its argument at runtime either way.
                if n.as_interpolated_symbol_node().is_some() {
                    return Ok(ArrayElem::Single(lower_node(result, hir, &n)?));
                }
                let name = alias_target_name(&n)?;
                Ok(ArrayElem::Single(hir.push(HirNode::SymbolLit(name))))
            })
            .collect::<PResult<Vec<_>>>()?;
        let send = hir.push(HirNode::Call {
            receiver: None,
            name: "undef_method".to_string(),
            args,
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        });
        // `Module#undef_method` answers the module; the `undef` KEYWORD answers
        // nil. The send is the mechanism, not the value -- so the value is
        // written back to the keyword's own.
        let nil = hir.push(HirNode::NilLit);
        return Ok(Some(hir.push(HirNode::Seq(vec![send, nil]))));
    }

    if let Some(def) = node.as_def_node() {
        let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        // `def self.name` (`DefNode::receiver()` is `Some(SelfNode)`) is a
        // class method; any OTHER explicit receiver (`def SomeConst.name`,
        // reopening a class from outside its own body) is a clean rejection
        // -- see `HirNode::DefMethod`'s docs.
        let is_class_method = match def.receiver() {
            None => false,
            Some(r) if r.as_self_node().is_some() => true,
            // `def SMTP.default_port` written INSIDE `class SMTP` is the older
            // spelling of `def self.default_port` -- net/smtp uses it
            // throughout -- so it has to register as a class method, not as a
            // runtime per-object singleton the compile-time tables never see
            // (a `class << self; alias a b` naming one couldn't resolve `b`).
            Some(r) if names_enclosing_class(hir, &r) => true,
            Some(r) => {
                // `def obj.name` on a NON-`self` receiver -- a
                // per-object singleton method. Desugar to a runtime install:
                //   RECV.define_singleton_method(:name, ->(params) { body })
                // A lambda body gives method-like strict arity and
                // `return`-exits-the-method semantics; `define_singleton_method`
                // rebinds `self` to RECV when the method runs (see
                // `runtime_meta::dynamic_from_proc`). Documented divergence: a
                // real `def` opens a FRESH scope, but the lambda closes over
                // enclosing locals -- so a body referencing an enclosing local
                // reads it here rather than raising `NameError` (rare; the
                // common `@ivar`/param/`self` uses are exact).
                let recv = lower_node(result, hir, &r)?;
                let params = lower_params(result, hir, def.parameters())?;
                let body = hir.in_def_body(|hir| lower_body(result, hir, def.body()))?;
                // A method-body lambda: its `yield`/`block_given?`/`&block`
                // reach the block the METHOD is called with, threaded through
                // `ProcData`'s call-site block slot (see `HirNode::Lambda`'s
                // `method_body`).
                let lambda = hir.push(HirNode::Lambda {
                    params: Box::new(params),
                    body,
                    method_body: true,
                });
                // Ruby labels a real `def`'s frame after the method however it
                // is installed, so this one is `name`, not a block. See
                // `Hir::singleton_def_names`.
                hir.singleton_def_names.insert(lambda, name.clone());
                let sym = hir.push(HirNode::SymbolLit(name));
                return Ok(Some(hir.push(HirNode::Call {
                    receiver: Some(recv),
                    name: "define_singleton_method".to_string(),
                    args: vec![ArrayElem::Single(sym), ArrayElem::Single(lambda)],
                    kwargs: vec![],
                    block: None,
                    block_arg: None,
                    safe: false,
                })));
            }
        };
        let params = lower_params(result, hir, def.parameters())?;
        let body = hir.in_def_body(|hir| lower_body(result, hir, def.body()))?;
        return Ok(Some(hir.push(HirNode::DefMethod {
            name,
            params: Box::new(params),
            body,
            is_class_method,
            // Only `lower_class_body_statement`'s own class-body-scoped
            // default-visibility tracking ever produces non-`Public` --
            // this generic path is reached for a top-level/nested `def`, or
            // one appearing as an ARGUMENT expression (`private def foo;
            // end` lowers its inner `def` through here, then
            // `lower_class_body_statement` retroactively mutates this same
            // node's `visibility` field once it sees the enclosing call).
            visibility: Visibility::Public,
            is_def: true,
        })));
    }

    // `class << obj` at expression/statement position -- top level or
    // inside a method body. Desugars to a sequence of per-object
    // `define_singleton_method` installs on the receiver; its value is the last
    // (Ruby's own rule, the last `def`'s symbol). `class << self` takes the
    // same route: the receiver lowers to `self` -- `main` at the top level, or
    // a method's own receiver inside a body -- and the runtime install attaches
    // the singleton to whatever object that is. (A `class << self` inside a
    // CLASS body is handled earlier by `lower_class_body`, defining class
    // methods; this generic path is only top-level/method-body.)
    if let Some(singleton) = node.as_singleton_class_node() {
        let stmts = desugar_singleton_class_defs(result, hir, &singleton)?;
        let seq = hir.push(HirNode::Seq(stmts));
        // The body's own `singleton class` frame, as in the class-body
        // mapping -- one whole `Seq` rather than a run of statements, since
        // this route keeps them together. Consulted only where the `Seq`
        // lands as a body STATEMENT; in expression position it is inert.
        hir.push_span(crate::lower::span_of(hir, node));
        let anchor = hir.push(HirNode::NilLit);
        hir.pop_span();
        hir.singleton_frame_stmts.insert(seq, anchor);
        return Ok(Some(seq));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every class-body directive has to have a runtime spelling: a class built
    /// at runtime runs its body as an ordinary block, where the static-path node
    /// has no meaning. `HirNode::is_class_body_directive` is the shared table,
    /// and it is exhaustive, so the only way to grow the directive set without
    /// tripping this test is to also teach `transform_runtime_class_body` the
    /// rewrite -- which is the point.
    #[test]
    fn every_class_body_directive_has_a_runtime_rewrite() {
        let directives = [
            HirNode::Include("M".to_string()),
            HirNode::Extend("M".to_string()),
            HirNode::Prepend("M".to_string()),
            HirNode::Undef(vec!["m".to_string()]),
            HirNode::AliasMethod {
                new_name: "a".to_string(),
                old_name: "b".to_string(),
                is_class_method: false,
            },
            HirNode::MethodVisibility {
                name: "m".to_string(),
                visibility: Visibility::Private,
            },
            HirNode::ModuleFunction("m".to_string()),
        ];
        for node in directives {
            assert!(
                node.is_class_body_directive(),
                "this test only covers directives"
            );
            let mut hir = Hir::default();
            let id = hir.push(node);
            let out = transform_runtime_class_body(&mut hir, vec![id])
                .expect("a directive rewrites rather than erroring");
            assert!(
                !hir[out[0]].is_class_body_directive(),
                "class-body directive left unrewritten for a runtime class body"
            );
        }
    }
}
