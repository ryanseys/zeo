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

        def self."parse" params "string, keep_script_lines: nil, error_tolerant: nil, keep_tokens: nil"(_recv, source, **opts) {
            let src = match source {
                RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
                other => {
                    return Err(crate::builtins::type_error!(
                        "wrong argument type {} (expected String)",
                        crate::builtins::check_type_name(other)
                    ));
                }
            };
            parse_to_node(&src, opts)
        }
        def self."parse_file" params "pathname, keep_script_lines: nil, error_tolerant: nil, keep_tokens: nil"(_recv, path, **opts) {
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
        def self."of" params "body, keep_script_lines: nil, error_tolerant: nil, keep_tokens: nil"(_recv, what, **_opts) {
            match what {
                RubyValue::Proc(_) => Err(raise_error("RuntimeError", PRISM_ERROR.to_string())),
                _ => Ok(RubyValue::Nil),
            }
        }
        def self."node_id_for_backtrace_location" params "backtrace_location"(_recv, _loc) {
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

    /// The byte range a translated node covers -- what a parent whose span
    /// runs to the END of its chain reads back off its tail.
    fn range_of(v: &RubyValue) -> Option<(usize, usize)> {
        let RubyValue::Object(o) = v else {
            return None;
        };
        o.as_any().downcast_ref::<RAstNode>().map(|n| n.byte_range)
    }

    /// A node's span extended to cover `tail` -- CRuby's `WHEN`/`RESBODY`
    /// chains each run to the end of everything after them.
    fn through(start: usize, own_end: usize, tail: &RubyValue) -> (usize, usize) {
        (
            start,
            range_of(tail).map_or(own_end, |(_, e)| e.max(own_end)),
        )
    }

    /// An `else` clause's body, or nil.
    fn else_body(
        clause: Option<&ruby_prism::ElseNode<'_>>,
        cx: &mut Cx,
        in_block: bool,
    ) -> RubyValue {
        match clause {
            Some(c) => opt_statements(c.statements(), cx, in_block),
            None => RubyValue::Nil,
        }
    }

    /// An EMPTY body, which CRuby renders as a `BEGIN` node holding one nil
    /// rather than as nothing.
    fn empty_begin(cx: &mut Cx, at: usize) -> RubyValue {
        node(cx, "BEGIN", at, at, vec![RubyValue::Nil])
    }

    /// The `when` arms from `idx` on, ending in `tail` (the `else` body, or
    /// nil).
    fn when_chain(arms: &[P<'_>], idx: usize, tail: RubyValue, cx: &mut Cx, b: bool) -> RubyValue {
        let Some(w) = arms.get(idx).and_then(P::as_when_node) else {
            return tail;
        };
        let loc = w.location();
        let conds: Vec<P<'_>> = w.conditions().iter().collect();
        // A lone splat stands alone; every other shape is a LIST.
        let head = if conds.len() == 1 && conds[0].as_splat_node().is_some() {
            translate(&conds[0], cx, b)
        } else {
            let lo = conds
                .first()
                .map_or(loc.start_offset(), |c| c.location().start_offset());
            let hi = conds
                .last()
                .map_or(loc.end_offset(), |c| c.location().end_offset());
            let elems: Vec<RubyValue> = conds.iter().map(|c| translate(c, cx, b)).collect();
            list_node(cx, lo, hi, elems)
        };
        let body = opt_statements(w.statements(), cx, b);
        let next = when_chain(arms, idx + 1, tail, cx, b);
        let (ws, we) = through(loc.start_offset(), loc.end_offset(), &next);
        node(cx, "WHEN", ws, we, vec![head, body, next])
    }

    /// [`when_chain`] for `case/in`'s arms.
    fn in_chain(arms: &[P<'_>], idx: usize, tail: RubyValue, cx: &mut Cx, b: bool) -> RubyValue {
        let Some(arm) = arms.get(idx).and_then(P::as_in_node) else {
            return tail;
        };
        let loc = arm.location();
        let pattern = translate(&arm.pattern(), cx, b);
        let body = opt_statements(arm.statements(), cx, b);
        let next = in_chain(arms, idx + 1, tail, cx, b);
        let (is, ie) = through(loc.start_offset(), loc.end_offset(), &next);
        node(cx, "IN", is, ie, vec![pattern, body, next])
    }

    /// A `begin` block: the body, wrapped in RESCUE when it has rescue
    /// clauses and in ENSURE when it has an ensure -- in that order, so
    /// `begin/rescue/ensure` is `ENSURE(RESCUE(..), ..)`.
    fn begin_body(
        x: &ruby_prism::BeginNode<'_>,
        cx: &mut Cx,
        b: bool,
        s: usize,
        _e: usize,
    ) -> RubyValue {
        // An empty body sits at the END of the keyword that opened it, and is
        // a `BEGIN` holding one nil rather than nothing.
        let body_at = x.begin_keyword_loc().map_or(s, |k| k.end_offset());
        let mut inner = match x.statements() {
            Some(stmts) if stmts.body().iter().next().is_some() => statements_body(&stmts, cx, b),
            _ => empty_begin(cx, body_at),
        };
        // A RESCUE/ENSURE spans from the BODY it wraps, never from `begin`.
        let start = range_of(&inner).map_or(s, |(st, _)| st);
        if let Some(r) = x.rescue_clause() {
            let els = else_body(x.else_clause().as_ref(), cx, b);
            let chain = resbody_chain(Some(r), cx, b);
            let end = range_of(&els)
                .or_else(|| range_of(&chain))
                .map_or(start, |(_, en)| en);
            inner = node(cx, "RESCUE", start, end, vec![inner, chain, els]);
        }
        if let Some(en) = x.ensure_clause() {
            let at = en.ensure_keyword_loc().end_offset();
            let body = match en.statements() {
                Some(stmts) if stmts.body().iter().next().is_some() => {
                    statements_body(&stmts, cx, b)
                }
                _ => empty_begin(cx, at),
            };
            let end = range_of(&body).map_or(start, |(_, en2)| en2);
            inner = node(cx, "ENSURE", start, end, vec![inner, body]);
        }
        inner
    }

    /// One `rescue` clause and everything after it: `[classes, binding, body,
    /// next]`, where the binding is the assignment `=> e` desugars to.
    fn resbody_chain(
        clause: Option<ruby_prism::RescueNode<'_>>,
        cx: &mut Cx,
        b: bool,
    ) -> RubyValue {
        let Some(r) = clause else {
            return RubyValue::Nil;
        };
        let loc = r.location();
        let excs: Vec<P<'_>> = r.exceptions().iter().collect();
        let classes = if excs.is_empty() {
            RubyValue::Nil
        } else {
            let lo = excs[0].location().start_offset();
            let hi = excs[excs.len() - 1].location().end_offset();
            let elems: Vec<RubyValue> = excs.iter().map(|c| translate(c, cx, b)).collect();
            list_node(cx, lo, hi, elems)
        };
        let binding = match r.reference() {
            Some(t) => error_binding(&t, r.operator_loc().map(|o| o.start_offset()), cx),
            None => RubyValue::Nil,
        };
        let body = match r.statements() {
            Some(stmts) if stmts.body().iter().next().is_some() => statements_body(&stmts, cx, b),
            _ => empty_begin(cx, loc.end_offset()),
        };
        let next = resbody_chain(r.subsequent(), cx, b);
        let (rs, re) = through(loc.start_offset(), loc.end_offset(), &next);
        node(cx, "RESBODY", rs, re, vec![classes, binding, body, next])
    }

    fn opt_translate(n: Option<&P<'_>>, cx: &mut Cx, b: bool) -> RubyValue {
        match n {
            Some(v) => translate(v, cx, b),
            None => RubyValue::Nil,
        }
    }

    /// A pattern's element LIST, or nil when there are none.
    fn pattern_list(elems: &[P<'_>], cx: &mut Cx, b: bool) -> RubyValue {
        if elems.is_empty() {
            return RubyValue::Nil;
        }
        let lo = elems[0].location().start_offset();
        let hi = elems[elems.len() - 1].location().end_offset();
        let kids: Vec<RubyValue> = elems.iter().map(|n| translate(n, cx, b)).collect();
        list_node(cx, lo, hi, kids)
    }

    /// A pattern's `*rest`: the binding it names, or the marker symbol for a
    /// nameless one.
    fn splat_target(rest: Option<&P<'_>>, cx: &mut Cx) -> RubyValue {
        let Some(r) = rest else {
            return RubyValue::Nil;
        };
        let Some(sp) = r.as_splat_node() else {
            return RubyValue::Nil;
        };
        // The binding spans the `*` too.
        let from = Some(r.location().start_offset());
        match sp.expression() {
            Some(t) => asgn_node(&t, from, RubyValue::Nil, cx),
            None => RubyValue::Symbol(crate::Symbol::intern("NODE_SPECIAL_NO_NAME_REST")),
        }
    }

    /// [`splat_target`] over an already-unwrapped splat expression.
    fn splat_target_of(expr: Option<P<'_>>, cx: &mut Cx) -> RubyValue {
        match expr {
            Some(t) => asgn_node(&t, None, RubyValue::Nil, cx),
            None => RubyValue::Symbol(crate::Symbol::intern("NODE_SPECIAL_NO_NAME_REST")),
        }
    }

    /// A pattern node's span: the extent of the parts it actually has,
    /// falling back to the whole construct when it has none (`in []`).
    fn pattern_span(loc: ruby_prism::Location<'_>, parts: &[&RubyValue]) -> (usize, usize) {
        let ranges: Vec<(usize, usize)> = parts.iter().filter_map(|p| range_of(p)).collect();
        match (
            ranges.iter().map(|r| r.0).min(),
            ranges.iter().map(|r| r.1).max(),
        ) {
            (Some(lo), Some(hi)) => (lo, hi),
            _ => (loc.start_offset(), loc.end_offset()),
        }
    }

    /// `rescue => e` is an ASSIGNMENT of `ERRINFO` in CRuby's tree, named
    /// after the target's kind.
    fn error_binding(target: &P<'_>, from: Option<usize>, cx: &mut Cx) -> RubyValue {
        let loc = target.location();
        let (s, e) = (from.unwrap_or(loc.start_offset()), loc.end_offset());
        let errinfo = node(cx, "ERRINFO", s, e, vec![]);
        asgn_node(target, from, errinfo, cx)
    }

    /// An assignment node named after the target's kind, holding `value` --
    /// `ERRINFO` for a `rescue => e`, nil for a pattern binding, which is how
    /// CRuby renders both.
    fn asgn_node(target: &P<'_>, from: Option<usize>, value: RubyValue, cx: &mut Cx) -> RubyValue {
        let loc = target.location();
        // A `rescue`'s assignment spans `=> e`, operator included.
        let (s, e) = (from.unwrap_or(loc.start_offset()), loc.end_offset());
        let (kind, name) = if let Some(t) = target.as_local_variable_target_node() {
            ("LASGN", t.name())
        } else if let Some(t) = target.as_instance_variable_target_node() {
            ("IASGN", t.name())
        } else if let Some(t) = target.as_global_variable_target_node() {
            ("GASGN", t.name())
        } else if let Some(t) = target.as_class_variable_target_node() {
            ("CVASGN", t.name())
        } else if let Some(t) = target.as_constant_target_node() {
            ("CDECL", t.name())
        } else {
            return RubyValue::Nil;
        };
        node(cx, kind, s, e, vec![sym_val(name.as_slice()), value])
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
                .unwrap_or_else(|| {
                    // No parameter list: CRuby puts the empty ARGS right after
                    // the method NAME.
                    let n = x.name_loc().end_offset();
                    (n, n)
                });
            let args = args_node(params, ps, pe, cx, false);
            // A `def` with its own `rescue`/`ensure` has a `begin` for a body.
            let body = match x.body() {
                Some(b) => match b.as_statements_node() {
                    Some(stmts) => statements_body(&stmts, cx, false),
                    None => translate(&b, cx, false),
                },
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
        // `case/when` -- CASE with a subject, CASE2 without. The whens are a
        // CHAIN: each WHEN's third child is the NEXT one, and the last one's
        // is the `else` body, which is how parse.y conses them up.
        // The `case/in` PATTERN family. Each is a fixed-shape node CRuby
        // builds in parse.y, and the shapes are not guessable: an absent
        // part is `nil`, a NAMELESS `*` is the symbol
        // `:NODE_SPECIAL_NO_NAME_REST`, and `**nil` is
        // `:NODE_SPECIAL_NO_REST_KEYWORD`.
        if let Some(x) = n.as_array_pattern_node() {
            let konst = opt_translate(x.constant().as_ref(), cx, in_block);
            let pre: Vec<P<'_>> = x.requireds().iter().collect();
            let post: Vec<P<'_>> = x.posts().iter().collect();
            let pre_list = pattern_list(&pre, cx, in_block);
            let rest = splat_target(x.rest().as_ref(), cx);
            let post_list = pattern_list(&post, cx, in_block);
            let (ps, pe) = pattern_span(x.location(), &[&konst, &pre_list, &rest, &post_list]);
            return node(cx, "ARYPTN", ps, pe, vec![konst, pre_list, rest, post_list]);
        }
        if let Some(x) = n.as_find_pattern_node() {
            let konst = opt_translate(x.constant().as_ref(), cx, in_block);
            let mid: Vec<P<'_>> = x.requireds().iter().collect();
            let left = splat_target_of(x.left().expression(), cx);
            let mid_list = pattern_list(&mid, cx, in_block);
            let right = splat_target(Some(&x.right()), cx);
            let (ps, pe) = pattern_span(x.location(), &[&konst, &left, &mid_list, &right]);
            return node(cx, "FNDPTN", ps, pe, vec![konst, left, mid_list, right]);
        }
        if let Some(x) = n.as_hash_pattern_node() {
            let konst = opt_translate(x.constant().as_ref(), cx, in_block);
            let pairs: Vec<P<'_>> = x.elements().iter().collect();
            let hash = if pairs.is_empty() {
                RubyValue::Nil
            } else {
                let mut kids = Vec::with_capacity(pairs.len() * 2);
                for a in &pairs {
                    let Some(a) = a.as_assoc_node() else { continue };
                    kids.push(translate(&a.key(), cx, in_block));
                    let v = a.value();
                    kids.push(match v.as_implicit_node() {
                        // `{a:}` -- the value is the binding the key implies,
                        // and it spans the KEY, colon included.
                        Some(i) => {
                            let inner = i.value();
                            match inner.as_local_variable_target_node() {
                                Some(t) => {
                                    let l = a.location();
                                    let errinfo = RubyValue::Nil;
                                    node(
                                        cx,
                                        "LASGN",
                                        l.start_offset(),
                                        l.end_offset(),
                                        vec![sym_val(t.name().as_slice()), errinfo],
                                    )
                                }
                                None => translate(&inner, cx, in_block),
                            }
                        }
                        None => translate(&v, cx, in_block),
                    });
                }
                let lo = pairs[0].location().start_offset();
                let hi = pairs[pairs.len() - 1].location().end_offset();
                let list = list_node(cx, lo, hi, kids);
                node(cx, "HASH", lo, hi, vec![list])
            };
            let rest = match x.rest() {
                // `**nil` -- "and no other keys", which is a marker, not a
                // binding.
                Some(r) if r.as_no_keywords_parameter_node().is_some() => {
                    RubyValue::Symbol(crate::Symbol::intern("NODE_SPECIAL_NO_REST_KEYWORD"))
                }
                Some(r) => match r.as_assoc_splat_node().and_then(|a| a.value()) {
                    Some(t) => asgn_node(&t, None, RubyValue::Nil, cx),
                    None => RubyValue::Nil,
                },
                None => RubyValue::Nil,
            };
            let (ps, pe) = pattern_span(x.location(), &[&konst, &hash, &rest]);
            return node(cx, "HSHPTN", ps, pe, vec![konst, hash, rest]);
        }
        if let Some(x) = n.as_alternation_pattern_node() {
            let l = translate(&x.left(), cx, in_block);
            let r = translate(&x.right(), cx, in_block);
            return node(cx, "OR", s, e, vec![l, r]);
        }
        // `Integer => n` -- CRuby renders a capture as a two-element HASH of
        // the pattern and the binding it feeds.
        if let Some(x) = n.as_capture_pattern_node() {
            let pat = translate(&x.value(), cx, in_block);
            let target = x.target().as_node();
            let bind = asgn_node(&target, None, RubyValue::Nil, cx);
            let list = list_node(cx, s, e, vec![pat, bind]);
            return node(cx, "HASH", s, e, vec![list]);
        }
        if let Some(x) = n.as_pinned_variable_node() {
            return translate(&x.variable(), cx, in_block);
        }
        // A bare name in a pattern BINDS; every other target kind does too.
        if n.as_local_variable_target_node().is_some()
            || n.as_instance_variable_target_node().is_some()
            || n.as_global_variable_target_node().is_some()
            || n.as_class_variable_target_node().is_some()
        {
            return asgn_node(n, None, RubyValue::Nil, cx);
        }
        // `*x` outside an argument list -- a `when *y`, a splatted assignment
        // right-hand side. One child: the expression.
        if let Some(x) = n.as_splat_node() {
            let inner = match x.expression() {
                Some(v) => translate(&v, cx, in_block),
                None => RubyValue::Nil,
            };
            return node(cx, "SPLAT", s, e, vec![inner]);
        }
        if let Some(x) = n.as_case_node() {
            let subject = match x.predicate() {
                Some(p) => translate(&p, cx, in_block),
                None => RubyValue::Nil,
            };
            let kind = if x.predicate().is_some() {
                "CASE"
            } else {
                "CASE2"
            };
            let els = else_body(x.else_clause().as_ref(), cx, in_block);
            let arms: Vec<P<'_>> = x.conditions().iter().collect();
            let chain = when_chain(&arms, 0, els, cx, in_block);
            return node(cx, kind, s, e, vec![subject, chain]);
        }
        // `case/in` is a different node kind all the way down: CASE3 over IN
        // arms, chained the same way.
        if let Some(x) = n.as_case_match_node() {
            let subject = match x.predicate() {
                Some(p) => translate(&p, cx, in_block),
                None => RubyValue::Nil,
            };
            let els = else_body(x.else_clause().as_ref(), cx, in_block);
            let arms: Vec<P<'_>> = x.conditions().iter().collect();
            let chain = in_chain(&arms, 0, els, cx, in_block);
            return node(cx, "CASE3", s, e, vec![subject, chain]);
        }
        if let Some(x) = n.as_begin_node() {
            return begin_body(&x, cx, in_block, s, e);
        }
        // `a rescue b` -- the modifier form is a RESCUE whose single RESBODY
        // names no exception class and binds nothing.
        if let Some(x) = n.as_rescue_modifier_node() {
            let body = translate(&x.expression(), cx, in_block);
            let handler = translate(&x.rescue_expression(), cx, in_block);
            let resbody = node(
                cx,
                "RESBODY",
                x.keyword_loc().start_offset(),
                e,
                vec![RubyValue::Nil, RubyValue::Nil, handler, RubyValue::Nil],
            );
            return node(cx, "RESCUE", s, e, vec![body, resbody, RubyValue::Nil]);
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
