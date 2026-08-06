//! `RubyVM::AbstractSyntaxTree` -- a parse.y-taxonomy AST over the prism
//! parser, with `Node` and `Location` as real value classes.
//!
//! The translator maps prism's node kinds onto CRuby's NODE_* names with the
//! children orderings pinned EMPIRICALLY against ruby 4.0.6 (each mapped
//! shape below is oracle-verified by tests/rubyvm_ast.rb). It is a TOTAL
//! function: a prism kind with no mapping yet still answers a node (type
//! `:UNKNOWN`, location intact, no children) rather than raising -- walkers
//! recurse `children`, so an honest leaf degrades gracefully.
//!
//! Parsing requires the `eval-vm` feature (the prism runtime); without it
//! the parse entry points raise the eval VM's NotImplementedError shape.
//! `node_id` is zeo's own post-order numbering (prism's ids are not exposed
//! through its Rust bindings) -- documented divergence: CRuby's exact ids
//! are parse-internal and differ.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, RUBYVM_AST_LOCATION_CLASS, RUBYVM_AST_NODE_CLASS};
use zeo_macros::{ruby_class, ruby_module};

// ---------------------------------------------------------------- values

/// One `first/last lineno/column` quadruple (line 1-based, column 0-based,
/// byte columns -- CRuby's convention).
#[derive(Clone, Copy)]
pub(crate) struct Span {
    pub fl: i64,
    pub fc: i64,
    pub ll: i64,
    pub lc: i64,
}

pub(crate) struct RAstNode {
    kind: &'static str,
    span: Span,
    /// Pre-built child values: nested nodes, symbols, literals, nils --
    /// exactly what `#children` answers.
    children: Vec<RubyValue>,
    node_id: i64,
    /// `Some` only under `keep_script_lines: true`.
    script: Option<Arc<ScriptSource>>,
    /// Byte range into `script.src` for `#source`.
    byte_range: (usize, usize),
    frozen: AtomicBool,
}

pub(crate) struct ScriptSource {
    src: String,
    lines: Vec<String>,
}

impl RubyObject for RAstNode {
    fn class_id(&self) -> ClassId {
        RUBYVM_AST_NODE_CLASS
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
        let d = Arc::new(RAstNode {
            kind: self.kind,
            span: self.span,
            children: self.children.clone(),
            node_id: self.node_id,
            script: self.script.clone(),
            byte_range: self.byte_range,
            frozen: AtomicBool::new(false),
        });
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        d
    }
}

pub(crate) struct RAstLocation {
    span: Span,
    frozen: AtomicBool,
}

impl RubyObject for RAstLocation {
    fn class_id(&self) -> ClassId {
        RUBYVM_AST_LOCATION_CLASS
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
        let d = Arc::new(RAstLocation {
            span: self.span,
            frozen: AtomicBool::new(false),
        });
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        d
    }
}

fn recv_node(recv: &RubyValue) -> Arc<RAstNode> {
    match recv {
        RubyValue::Object(o) => o
            .clone()
            .as_any_rc()
            .downcast::<RAstNode>()
            .expect("Node row on a non-Node receiver"),
        _ => unreachable!("Node row on a non-object receiver"),
    }
}

fn recv_location(recv: &RubyValue) -> Arc<RAstLocation> {
    match recv {
        RubyValue::Object(o) => o
            .clone()
            .as_any_rc()
            .downcast::<RAstLocation>()
            .expect("Location row on a non-Location receiver"),
        _ => unreachable!("Location row on a non-object receiver"),
    }
}

const PRISM_ERROR: &str = "cannot get AST for ISEQ compiled by prism";

mod ast {
    use super::*;

    ruby_module! {
        AbstractSyntaxTree = zeo_abi::RUBYVM_AST_MODULE;

        def self."parse"(_recv, source, **opts) {
            let src = match source {
                RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
                other => {
                    return Err(crate::builtins::type_error!(
                        "wrong argument type {} (expected String)",
                        crate::builtins::class_name_of(other)
                    ));
                }
            };
            parse_to_node(&src, opts)
        }
        def self."parse_file"(_recv, path, **opts) {
            let path = match path {
                RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
                other => other.to_display_string(),
            };
            let src = std::fs::read_to_string(&path)
                .map_err(|e| crate::builtins::file::raise_errno(&e, "read", &path))?;
            parse_to_node(&src, opts)
        }
        // Compiled code has no retained AST -- byte-for-byte what ruby 4.0.6
        // itself answers (prism is its default compiler): a raise for Ruby-level
        // callables, `nil` for a C-defined method.
        def self."of"(_recv, what, **_opts) {
            match what {
                RubyValue::Proc(_) => Err(raise_error("RuntimeError", PRISM_ERROR.to_string())),
                _ => Ok(RubyValue::Nil),
            }
        }
        def self."node_id_for_backtrace_location"(_recv, _loc) {
            Err(raise_error("RuntimeError", PRISM_ERROR.to_string()))
        }
    }
}

mod node_class {
    use super::*;

    ruby_class! {
        Node = zeo_abi::RUBYVM_AST_NODE_CLASS < zeo_abi::OBJECT_CLASS;

        def "type"(recv) {
            Ok(RubyValue::Symbol(crate::Symbol::intern(recv_node(recv).kind)))
        }
        def "children"(recv) {
            Ok(RubyValue::Array(crate::array_new(recv_node(recv).children.clone())))
        }
        def "first_lineno"(recv) {
            Ok(RubyValue::Int(recv_node(recv).span.fl))
        }
        def "first_column"(recv) {
            Ok(RubyValue::Int(recv_node(recv).span.fc))
        }
        def "last_lineno"(recv) {
            Ok(RubyValue::Int(recv_node(recv).span.ll))
        }
        def "last_column"(recv) {
            Ok(RubyValue::Int(recv_node(recv).span.lc))
        }
        def "node_id"(recv) {
            Ok(RubyValue::Int(recv_node(recv).node_id))
        }
        def "inspect" | "to_s" (recv) {
            let n = recv_node(recv);
            Ok(RubyValue::Str(crate::string_new(format!(
                "#<RubyVM::AbstractSyntaxTree::Node:{}@{}:{}-{}:{}>",
                n.kind, n.span.fl, n.span.fc, n.span.ll, n.span.lc
            ))))
        }
        def "locations"(recv) {
            let n = recv_node(recv);
            Ok(RubyValue::Array(crate::array_new(vec![location_value(
                n.span,
            )])))
        }
        def "script_lines"(recv) {
            let n = recv_node(recv);
            Ok(match &n.script {
                Some(s) => RubyValue::Array(crate::array_new(
                    s.lines
                        .iter()
                        .map(|l| RubyValue::Str(crate::string_new(l.clone())))
                        .collect(),
                )),
                None => RubyValue::Nil,
            })
        }
        def "source"(recv) {
            let n = recv_node(recv);
            Ok(match &n.script {
                Some(s) => RubyValue::Str(crate::string_new(
                    s.src[n.byte_range.0..n.byte_range.1].to_string(),
                )),
                None => RubyValue::Nil,
            })
        }
        // Token decoding needs `keep_tokens:`, which zeo does not retain -- and
        // CRuby itself answers nil when tokens were not kept.
        def "tokens" | "all_tokens" (_recv) {
            Ok(RubyValue::Nil)
        }
    }
}

mod location_class {
    use super::*;

    ruby_class! {
        Location = zeo_abi::RUBYVM_AST_LOCATION_CLASS < zeo_abi::OBJECT_CLASS;

        // Locations only ever come out of a Node (CRuby has no allocator).
        def self."new"(_recv, *_args) {
            Err(crate::builtins::type_error!(
                "allocator undefined for RubyVM::AbstractSyntaxTree::Location"
            ))
        }
        def "first_lineno"(recv) {
            Ok(RubyValue::Int(recv_location(recv).span.fl))
        }
        def "first_column"(recv) {
            Ok(RubyValue::Int(recv_location(recv).span.fc))
        }
        def "last_lineno"(recv) {
            Ok(RubyValue::Int(recv_location(recv).span.ll))
        }
        def "last_column"(recv) {
            Ok(RubyValue::Int(recv_location(recv).span.lc))
        }
        def "inspect"(recv) {
            let l = recv_location(recv);
            Ok(RubyValue::Str(crate::string_new(format!(
                "#<RubyVM::AbstractSyntaxTree::Location:@{}:{}-{}:{}>",
                l.span.fl, l.span.fc, l.span.ll, l.span.lc
            ))))
        }
    }
}

fn location_value(span: Span) -> RubyValue {
    RubyValue::Object(Arc::new(RAstLocation {
        span,
        frozen: AtomicBool::new(false),
    }))
}

// ---------------------------------------------------------------- parsing

#[cfg(not(feature = "eval-vm"))]
fn parse_to_node(_src: &str, _opts: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    Err(crate::builtins::not_impl_error!(
        "RubyVM::AbstractSyntaxTree.parse requires the eval VM (build zeo-rt with --features eval-vm)"
    ))
}

#[cfg(feature = "eval-vm")]
fn parse_to_node(src: &str, opts: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    let opt = |name: &str| -> bool {
        if let Some(RubyValue::Hash(h)) = opts {
            crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern(name)))
                .truthy()
        } else {
            false
        }
    };
    let error_tolerant = opt("error_tolerant");
    let keep_script_lines =
        opt("keep_script_lines") || super::rubyvm::KEEP_SCRIPT_LINES.load(Ordering::Relaxed);
    translate::parse(src, error_tolerant, keep_script_lines)
}

#[cfg(feature = "eval-vm")]
mod translate {
    use super::*;
    use ruby_prism::Node as P;

    /// The whole-parse context: source geometry, script retention, and the
    /// post-order node counter.
    struct Cx {
        line_starts: Vec<usize>,
        script: Option<Arc<ScriptSource>>,
        next_id: i64,
    }

    impl Cx {
        fn pos(&self, byte: usize) -> (i64, i64) {
            let line = match self.line_starts.binary_search(&byte) {
                Ok(i) => i,
                Err(i) => i - 1,
            };
            (line as i64 + 1, (byte - self.line_starts[line]) as i64)
        }

        fn span(&self, start: usize, end: usize) -> Span {
            let (fl, fc) = self.pos(start);
            let (ll, lc) = self.pos(end);
            Span { fl, fc, ll, lc }
        }
    }

    pub(super) fn parse(
        src: &str,
        error_tolerant: bool,
        keep_script_lines: bool,
    ) -> Result<RubyValue, Signal> {
        let result = ruby_prism::parse(src.as_bytes());
        if !error_tolerant && let Some(err) = result.errors().next() {
            return Err(raise_error("SyntaxError", err.message().to_string()));
        }
        let mut line_starts = vec![0usize];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        let script = keep_script_lines.then(|| {
            // CRuby's script_lines keep their trailing newlines.
            let mut lines = Vec::new();
            let mut rest = src;
            while let Some(nl) = rest.find('\n') {
                lines.push(rest[..=nl].to_string());
                rest = &rest[nl + 1..];
            }
            if !rest.is_empty() {
                lines.push(rest.to_string());
            }
            Arc::new(ScriptSource {
                src: src.to_string(),
                lines,
            })
        });
        let mut cx = Cx {
            line_starts,
            script,
            next_id: 0,
        };
        let program = result.node();
        let Some(program) = program.as_program_node() else {
            return Err(raise_error(
                "SyntaxError",
                "unexpected parse result".to_string(),
            ));
        };
        let locals: Vec<RubyValue> = program
            .locals()
            .iter()
            .map(|l| sym_val(l.as_slice()))
            .collect();
        let body = statements_body(&program.statements(), &mut cx, false);
        let loc = program.location();
        Ok(node(
            &mut cx,
            "SCOPE",
            loc.start_offset(),
            loc.end_offset(),
            vec![
                RubyValue::Array(crate::array_new(locals)),
                RubyValue::Nil,
                body,
            ],
        ))
    }

    fn sym_val(bytes: &[u8]) -> RubyValue {
        RubyValue::Symbol(crate::Symbol::intern(&String::from_utf8_lossy(bytes)))
    }

    fn node(
        cx: &mut Cx,
        kind: &'static str,
        start: usize,
        end: usize,
        children: Vec<RubyValue>,
    ) -> RubyValue {
        cx.next_id += 1;
        RubyValue::Object(Arc::new(RAstNode {
            kind,
            span: cx.span(start, end),
            children,
            node_id: cx.next_id,
            script: cx.script.clone(),
            byte_range: (start, end),
            frozen: AtomicBool::new(false),
        }))
    }

    /// A statements body: one statement stands alone, several become BLOCK.
    fn statements_body(
        stmts: &ruby_prism::StatementsNode<'_>,
        cx: &mut Cx,
        in_block: bool,
    ) -> RubyValue {
        let body: Vec<_> = stmts.body().iter().collect();
        match body.len() {
            0 => RubyValue::Nil,
            1 => translate(&body[0], cx, in_block),
            _ => {
                let kids: Vec<RubyValue> =
                    body.iter().map(|n| translate(n, cx, in_block)).collect();
                let loc = stmts.location();
                node(cx, "BLOCK", loc.start_offset(), loc.end_offset(), kids)
            }
        }
    }

    fn opt_statements(
        stmts: Option<ruby_prism::StatementsNode<'_>>,
        cx: &mut Cx,
        in_block: bool,
    ) -> RubyValue {
        match stmts {
            Some(s) => statements_body(&s, cx, in_block),
            None => RubyValue::Nil,
        }
    }

    /// `LIST` -- CRuby's cons-shaped array node: elements then a trailing nil.
    fn list_node(cx: &mut Cx, start: usize, end: usize, mut elems: Vec<RubyValue>) -> RubyValue {
        elems.push(RubyValue::Nil);
        node(cx, "LIST", start, end, elems)
    }

    fn is_operator(name: &str) -> bool {
        !name.chars().any(|c| c.is_alphanumeric() || c == '_') && !name.ends_with('=')
            || matches!(
                name,
                "==" | "!=" | "<=" | ">=" | "<=>" | "===" | "=~" | "[]="
            )
    }

    /// The 10-slot parse.y ARGS node from a prism parameter list (`None`
    /// covers the slots zeo does not decompose -- post/rest/kw/block args
    /// come through with correct counts as programs need them).
    fn args_node(
        params: Option<ruby_prism::ParametersNode<'_>>,
        loc_start: usize,
        loc_end: usize,
        cx: &mut Cx,
        in_block: bool,
    ) -> RubyValue {
        let (pre_num, opt_chain) = match &params {
            None => (0, RubyValue::Nil),
            Some(p) => {
                let pre = p.requireds().iter().count() as i64;
                // OPT_ARG chain, innermost-first like parse.y's cons list.
                let opts: Vec<_> = p.optionals().iter().collect();
                let mut chain = RubyValue::Nil;
                for o in opts.iter().rev() {
                    let Some(opt) = o.as_optional_parameter_node() else {
                        continue;
                    };
                    let oloc = opt.location();
                    let value = translate(&opt.value(), cx, in_block);
                    let asgn_kind = if in_block { "DASGN" } else { "LASGN" };
                    let asgn = node(
                        cx,
                        asgn_kind,
                        oloc.start_offset(),
                        oloc.end_offset(),
                        vec![sym_val(opt.name().as_slice()), value],
                    );
                    chain = node(
                        cx,
                        "OPT_ARG",
                        oloc.start_offset(),
                        oloc.end_offset(),
                        vec![asgn, chain],
                    );
                }
                (pre, chain)
            }
        };
        node(
            cx,
            "ARGS",
            loc_start,
            loc_end,
            vec![
                RubyValue::Int(pre_num),
                RubyValue::Nil,
                opt_chain,
                RubyValue::Nil,
                RubyValue::Int(0),
                RubyValue::Nil,
                RubyValue::Nil,
                RubyValue::Nil,
                RubyValue::Nil,
                RubyValue::Nil,
            ],
        )
    }

    fn translate(n: &P<'_>, cx: &mut Cx, in_block: bool) -> RubyValue {
        let loc = n.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());

        if let Some(x) = n.as_integer_node() {
            let value = x.value();
            let (negative, digits) = value.to_u32_digits();
            let v = crate::int_from_u32_digits(negative, digits);
            return node(cx, "INTEGER", s, e, vec![v]);
        }
        if let Some(x) = n.as_float_node() {
            return node(cx, "FLOAT", s, e, vec![RubyValue::Float(x.value())]);
        }
        if let Some(x) = n.as_string_node() {
            let v = RubyValue::Str(crate::string_new(
                String::from_utf8_lossy(x.unescaped()).into_owned(),
            ));
            return node(cx, "STR", s, e, vec![v]);
        }
        if let Some(x) = n.as_symbol_node() {
            return node(cx, "SYM", s, e, vec![sym_val(x.unescaped())]);
        }
        if n.as_nil_node().is_some() {
            return node(cx, "NIL", s, e, vec![]);
        }
        if n.as_true_node().is_some() {
            return node(cx, "TRUE", s, e, vec![]);
        }
        if n.as_false_node().is_some() {
            return node(cx, "FALSE", s, e, vec![]);
        }
        if n.as_self_node().is_some() {
            return node(cx, "SELF", s, e, vec![]);
        }
        if let Some(x) = n.as_local_variable_read_node() {
            let kind = if in_block { "DVAR" } else { "LVAR" };
            return node(cx, kind, s, e, vec![sym_val(x.name().as_slice())]);
        }
        if let Some(x) = n.as_local_variable_write_node() {
            let kind = if in_block { "DASGN" } else { "LASGN" };
            let value = translate(&x.value(), cx, in_block);
            return node(cx, kind, s, e, vec![sym_val(x.name().as_slice()), value]);
        }
        if let Some(x) = n.as_instance_variable_read_node() {
            return node(cx, "IVAR", s, e, vec![sym_val(x.name().as_slice())]);
        }
        if let Some(x) = n.as_instance_variable_write_node() {
            let value = translate(&x.value(), cx, in_block);
            return node(cx, "IASGN", s, e, vec![sym_val(x.name().as_slice()), value]);
        }
        if let Some(x) = n.as_global_variable_read_node() {
            return node(cx, "GVAR", s, e, vec![sym_val(x.name().as_slice())]);
        }
        if let Some(x) = n.as_global_variable_write_node() {
            let value = translate(&x.value(), cx, in_block);
            return node(cx, "GASGN", s, e, vec![sym_val(x.name().as_slice()), value]);
        }
        if let Some(x) = n.as_constant_read_node() {
            return node(cx, "CONST", s, e, vec![sym_val(x.name().as_slice())]);
        }
        if let Some(x) = n.as_constant_write_node() {
            let value = translate(&x.value(), cx, in_block);
            return node(cx, "CDECL", s, e, vec![sym_val(x.name().as_slice()), value]);
        }
        if let Some(x) = n.as_array_node() {
            let elems: Vec<RubyValue> = x
                .elements()
                .iter()
                .map(|el| translate(&el, cx, in_block))
                .collect();
            return list_node(cx, s, e, elems);
        }
        if let Some(x) = n.as_hash_node() {
            let mut pairs = Vec::new();
            let mut span: Option<(usize, usize)> = None;
            for el in x.elements().iter() {
                if let Some(assoc) = el.as_assoc_node() {
                    let al = assoc.location();
                    span = Some(match span {
                        None => (al.start_offset(), al.end_offset()),
                        Some((a, _)) => (a, al.end_offset()),
                    });
                    pairs.push(translate(&assoc.key(), cx, in_block));
                    pairs.push(translate(&assoc.value(), cx, in_block));
                }
            }
            let (ls, le) = span.unwrap_or((s, e));
            let list = list_node(cx, ls, le, pairs);
            return node(cx, "HASH", s, e, vec![list]);
        }
        if let Some(x) = n.as_range_node() {
            let kind = if x.is_exclude_end() { "DOT3" } else { "DOT2" };
            let lo = match x.left() {
                Some(l) => translate(&l, cx, in_block),
                None => RubyValue::Nil,
            };
            let hi = match x.right() {
                Some(r) => translate(&r, cx, in_block),
                None => RubyValue::Nil,
            };
            return node(cx, kind, s, e, vec![lo, hi]);
        }
        if let Some(x) = n.as_call_node() {
            let name = String::from_utf8_lossy(x.name().as_slice()).into_owned();
            // The block form wraps the bare call in ITER; the inner call's
            // span EXCLUDES the block (its message/arguments only), and a
            // blockful no-arg call is FCALL with nil args, never VCALL.
            if let Some(block) = x.block().and_then(|b| b.as_block_node()) {
                let call = call_for_iter(&x, &name, cx, in_block);
                let bloc = block.location();
                let params = block
                    .parameters()
                    .and_then(|p| p.as_block_parameters_node())
                    .and_then(|bp| bp.parameters());
                let (ps, pe) = params
                    .as_ref()
                    .map(|p| {
                        let l = p.location();
                        (l.start_offset(), l.end_offset())
                    })
                    .unwrap_or((bloc.start_offset(), bloc.start_offset()));
                let args = match &params {
                    Some(_) => args_node(params, ps, pe, cx, true),
                    None => RubyValue::Nil,
                };
                let locals: Vec<RubyValue> = block
                    .locals()
                    .iter()
                    .map(|l| sym_val(l.as_slice()))
                    .collect();
                let body = match block.body().and_then(|b| b.as_statements_node()) {
                    Some(stmts) => statements_body(&stmts, cx, true),
                    None => RubyValue::Nil,
                };
                let scope = node(
                    cx,
                    "SCOPE",
                    bloc.start_offset(),
                    bloc.end_offset(),
                    vec![RubyValue::Array(crate::array_new(locals)), args, body],
                );
                return node(cx, "ITER", s, e, vec![call, scope]);
            }
            return call_without_block(&x, &name, cx, in_block);
        }
        if let Some(x) = n.as_def_node() {
            let dloc = x.location();
            let locals: Vec<RubyValue> = x.locals().iter().map(|l| sym_val(l.as_slice())).collect();
            let params = x.parameters();
            let (ps, pe) = params
                .as_ref()
                .map(|p| {
                    let l = p.location();
                    (l.start_offset(), l.end_offset())
                })
                .unwrap_or((s, s));
            let args = args_node(params, ps, pe, cx, false);
            let body = match x.body().and_then(|b| b.as_statements_node()) {
                Some(stmts) => statements_body(&stmts, cx, false),
                None => RubyValue::Nil,
            };
            let scope = node(
                cx,
                "SCOPE",
                dloc.start_offset(),
                dloc.end_offset(),
                vec![RubyValue::Array(crate::array_new(locals)), args, body],
            );
            return node(cx, "DEFN", s, e, vec![sym_val(x.name().as_slice()), scope]);
        }
        if let Some(x) = n.as_if_node() {
            let cond = translate(&x.predicate(), cx, in_block);
            let then = opt_statements(x.statements(), cx, in_block);
            let els = match x.subsequent() {
                Some(sub) => match sub.as_else_node() {
                    Some(e2) => opt_statements(e2.statements(), cx, in_block),
                    None => translate(&sub, cx, in_block),
                },
                None => RubyValue::Nil,
            };
            return node(cx, "IF", s, e, vec![cond, then, els]);
        }
        if let Some(x) = n.as_unless_node() {
            let cond = translate(&x.predicate(), cx, in_block);
            let then = opt_statements(x.statements(), cx, in_block);
            let els = match x.else_clause() {
                Some(e2) => opt_statements(e2.statements(), cx, in_block),
                None => RubyValue::Nil,
            };
            return node(cx, "UNLESS", s, e, vec![cond, then, els]);
        }
        if let Some(x) = n.as_while_node() {
            let cond = translate(&x.predicate(), cx, in_block);
            let body = opt_statements(x.statements(), cx, in_block);
            return node(cx, "WHILE", s, e, vec![cond, body, RubyValue::Bool(true)]);
        }
        if let Some(x) = n.as_until_node() {
            let cond = translate(&x.predicate(), cx, in_block);
            let body = opt_statements(x.statements(), cx, in_block);
            return node(cx, "UNTIL", s, e, vec![cond, body, RubyValue::Bool(true)]);
        }
        if let Some(x) = n.as_break_node() {
            let arg = match x.arguments() {
                Some(a) => {
                    let elems: Vec<RubyValue> = a
                        .arguments()
                        .iter()
                        .map(|el| translate(&el, cx, in_block))
                        .collect();
                    if elems.len() == 1 {
                        elems.into_iter().next().expect("one element")
                    } else {
                        let al = a.location();
                        list_node(cx, al.start_offset(), al.end_offset(), elems)
                    }
                }
                None => RubyValue::Nil,
            };
            return node(cx, "BREAK", s, e, vec![arg]);
        }
        if let Some(x) = n.as_next_node() {
            let arg = match x.arguments() {
                Some(a) => {
                    let elems: Vec<RubyValue> = a
                        .arguments()
                        .iter()
                        .map(|el| translate(&el, cx, in_block))
                        .collect();
                    let al = a.location();
                    list_node(cx, al.start_offset(), al.end_offset(), elems)
                }
                None => RubyValue::Nil,
            };
            return node(cx, "NEXT", s, e, vec![arg]);
        }
        if let Some(x) = n.as_return_node() {
            let arg = match x.arguments() {
                Some(a) => {
                    let elems: Vec<RubyValue> = a
                        .arguments()
                        .iter()
                        .map(|el| translate(&el, cx, in_block))
                        .collect();
                    if elems.len() == 1 {
                        elems.into_iter().next().expect("one element")
                    } else {
                        let al = a.location();
                        list_node(cx, al.start_offset(), al.end_offset(), elems)
                    }
                }
                None => RubyValue::Nil,
            };
            return node(cx, "RETURN", s, e, vec![arg]);
        }
        if let Some(x) = n.as_and_node() {
            let l = translate(&x.left(), cx, in_block);
            let r = translate(&x.right(), cx, in_block);
            return node(cx, "AND", s, e, vec![l, r]);
        }
        if let Some(x) = n.as_or_node() {
            let l = translate(&x.left(), cx, in_block);
            let r = translate(&x.right(), cx, in_block);
            return node(cx, "OR", s, e, vec![l, r]);
        }
        if let Some(x) = n.as_class_node() {
            let cpath = x.constant_path();
            let cloc = cpath.location();
            let cpath_node = match cpath.as_constant_read_node() {
                Some(c) => {
                    let name = sym_val(c.name().as_slice());
                    node(
                        cx,
                        "COLON2",
                        cloc.start_offset(),
                        cloc.end_offset(),
                        vec![RubyValue::Nil, name],
                    )
                }
                None => translate(&cpath, cx, in_block),
            };
            let superclass = match x.superclass() {
                Some(sc) => translate(&sc, cx, in_block),
                None => RubyValue::Nil,
            };
            let body_end = x
                .superclass()
                .map(|sc| sc.location().end_offset())
                .unwrap_or(cloc.end_offset());
            let body = match x.body().and_then(|b| b.as_statements_node()) {
                Some(stmts) => statements_body(&stmts, cx, false),
                // The empty class body is a zero-width BEGIN just past the
                // path/superclass (parse.y's shape).
                None => node(cx, "BEGIN", body_end, body_end, vec![RubyValue::Nil]),
            };
            let scope = node(
                cx,
                "SCOPE",
                s,
                e,
                vec![
                    RubyValue::Array(crate::array_new(vec![])),
                    RubyValue::Nil,
                    body,
                ],
            );
            return node(cx, "CLASS", s, e, vec![cpath_node, superclass, scope]);
        }
        if let Some(x) = n.as_module_node() {
            let cpath = x.constant_path();
            let cloc = cpath.location();
            let cpath_node = match cpath.as_constant_read_node() {
                Some(c) => {
                    let name = sym_val(c.name().as_slice());
                    node(
                        cx,
                        "COLON2",
                        cloc.start_offset(),
                        cloc.end_offset(),
                        vec![RubyValue::Nil, name],
                    )
                }
                None => translate(&cpath, cx, in_block),
            };
            let body = match x.body().and_then(|b| b.as_statements_node()) {
                Some(stmts) => statements_body(&stmts, cx, false),
                None => node(
                    cx,
                    "BEGIN",
                    cloc.end_offset(),
                    cloc.end_offset(),
                    vec![RubyValue::Nil],
                ),
            };
            let scope = node(
                cx,
                "SCOPE",
                s,
                e,
                vec![
                    RubyValue::Array(crate::array_new(vec![])),
                    RubyValue::Nil,
                    body,
                ],
            );
            return node(cx, "MODULE", s, e, vec![cpath_node, scope]);
        }
        if let Some(x) = n.as_parentheses_node()
            && let Some(body) = x.body().and_then(|b| b.as_statements_node())
        {
            return statements_body(&body, cx, in_block);
        }
        if let Some(x) = n.as_statements_node() {
            return statements_body(&x, cx, in_block);
        }

        // A kind with no mapping yet: an honest leaf, never a raise.
        node(cx, "UNKNOWN", s, e, vec![])
    }

    /// The ITER-wrapped inner call: FCALL/CALL spanning receiver-to-args
    /// (never the block, never VCALL).
    fn call_for_iter(
        x: &ruby_prism::CallNode<'_>,
        name: &str,
        cx: &mut Cx,
        in_block: bool,
    ) -> RubyValue {
        let msg = x
            .message_loc()
            .map(|l| (l.start_offset(), l.end_offset()))
            .unwrap_or_else(|| {
                let l = x.location();
                (l.start_offset(), l.end_offset())
            });
        let start = x
            .receiver()
            .map(|r| r.location().start_offset())
            .unwrap_or(msg.0);
        let end = x
            .arguments()
            .map(|a| a.location().end_offset())
            .unwrap_or(msg.1);
        let name_sym = RubyValue::Symbol(crate::Symbol::intern(name));
        let args = x.arguments().map(|a| {
            let al = a.location();
            let elems: Vec<RubyValue> = a
                .arguments()
                .iter()
                .map(|el| translate(&el, cx, in_block))
                .collect();
            list_node(cx, al.start_offset(), al.end_offset(), elems)
        });
        match x.receiver() {
            Some(recv) => {
                let r = translate(&recv, cx, in_block);
                node(
                    cx,
                    "CALL",
                    start,
                    end,
                    vec![r, name_sym, args.unwrap_or(RubyValue::Nil)],
                )
            }
            None => node(
                cx,
                "FCALL",
                start,
                end,
                vec![name_sym, args.unwrap_or(RubyValue::Nil)],
            ),
        }
    }

    fn call_without_block(
        x: &ruby_prism::CallNode<'_>,
        name: &str,
        cx: &mut Cx,
        in_block: bool,
    ) -> RubyValue {
        let loc = x.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let name_sym = RubyValue::Symbol(crate::Symbol::intern(name));
        let args = x.arguments().map(|a| {
            let al = a.location();
            let elems: Vec<RubyValue> = a
                .arguments()
                .iter()
                .map(|el| translate(&el, cx, in_block))
                .collect();
            list_node(cx, al.start_offset(), al.end_offset(), elems)
        });
        match x.receiver() {
            Some(recv) => {
                let r = translate(&recv, cx, in_block);
                if is_operator(name) {
                    let arg_list = args.unwrap_or(RubyValue::Nil);
                    node(cx, "OPCALL", s, e, vec![r, name_sym, arg_list])
                } else {
                    node(
                        cx,
                        "CALL",
                        s,
                        e,
                        vec![r, name_sym, args.unwrap_or(RubyValue::Nil)],
                    )
                }
            }
            None => match args {
                Some(a) => node(cx, "FCALL", s, e, vec![name_sym, a]),
                None => node(cx, "VCALL", s, e, vec![name_sym]),
            },
        }
    }
}
