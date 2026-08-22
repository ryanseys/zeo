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
    /// What a construct's grammar eats between its header and its first
    /// statement -- see [`Cx::leading_terminator`], which is where the
    /// measurements behind these three live.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Absorb {
        /// Nothing: a `class`/`module` body with no superclass clause, the one
        /// place CRuby's grammar leaves even a newline in the statements list.
        None,
        /// A run of newlines, never a `;`: `begin`, a block body, a
        /// parenthesised list.
        Newlines,
        /// One terminator of either kind: `def`'s arglist, `while`'s `do`,
        /// `if`'s `then`. Only a SECOND separator survives there.
        One,
    }

    struct Cx {
        line_starts: Vec<usize>,
        script: Option<Arc<ScriptSource>>,
        next_id: i64,
        /// The source bytes, for the handful of spans CRuby measures by
        /// TERMINATOR rather than by node -- see [`Cx::through_terminator`].
        src: Vec<u8>,
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
            let (ll, lc) = self.end_pos(end);
            Span { fl, fc, ll, lc }
        }

        /// [`Cx::pos`] for an END offset. CRuby does not roll one that sits
        /// exactly on a line start onto the next line: a heredoc part ending
        /// AT its newline reports `2,8` -- a column past that line's last
        /// character -- where the start convention would say `3,0`.
        fn end_pos(&self, byte: usize) -> (i64, i64) {
            if byte > 0
                && let Ok(line) = self.line_starts.binary_search(&byte)
                && line > 0
            {
                return (line as i64, (byte - self.line_starts[line - 1]) as i64);
            }
            self.pos(byte)
        }

        /// The offset of a leading terminator the grammar did NOT absorb.
        ///
        /// CRuby's `stmts: none` reduces to `NEW_BEGIN(0)` before the first
        /// real statement, so a body whose text starts with a terminator
        /// carries a zero-width `BEGIN nil` ahead of it. Which terminators
        /// survive is a grammar fact, measured against the oracle: a `\n` is
        /// absorbed everywhere it can be (`def`'s arglist, `while`'s `do`,
        /// `if`'s `then`, a `class ... < Super`'s term, a block's parameter
        /// list) while a `;` never is -- except in a `class`/`module` body
        /// with NO superclass clause, where nothing is there to take either.
        fn leading_gap(&self, from: usize, absorb: Absorb) -> Option<usize> {
            let space = |mut i: usize| {
                while matches!(self.src.get(i), Some(b' ' | b'\t' | b'\r')) {
                    i += 1;
                }
                i
            };
            // Where the statements list starts parsing: past whatever the
            // grammar took, and NOT past the whitespace after it -- CRuby
            // sites the empty statement exactly there.
            let mut at = from;
            match absorb {
                Absorb::None => {}
                Absorb::Newlines => {
                    let mut i = space(at);
                    while matches!(self.src.get(i), Some(b'\n')) {
                        at = i + 1;
                        i = space(at);
                    }
                }
                Absorb::One => {
                    let i = space(at);
                    if matches!(self.src.get(i), Some(b';' | b'\n')) {
                        at = i + 1;
                    }
                }
            }
            match self.src.get(space(at)) {
                Some(b';') => Some(at),
                Some(b'\n') if absorb == Absorb::None => Some(at),
                _ => None,
            }
        }

        /// `end`, plus a `;` sitting immediately after it.
        ///
        /// A `when`/`in` arm runs to the SEPARATOR in CRuby, not to its last
        /// statement -- `when 1 then 2; end` ends past the `;` while
        /// `when 1 then 2 end` ends at the `2`. prism has no node for the
        /// separator, so the byte is what says which of the two this is.
        fn through_terminator(&self, end: usize) -> usize {
            match self.src.get(end) {
                Some(b';') => end + 1,
                _ => end,
            }
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
            src: src.as_bytes().to_vec(),
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
            1 => translate_stmt(&body[0], cx, in_block),
            _ => {
                let kids: Vec<RubyValue> = body
                    .iter()
                    .map(|n| translate_stmt(n, cx, in_block))
                    .collect();
                let loc = stmts.location();
                node(cx, "BLOCK", loc.start_offset(), loc.end_offset(), kids)
            }
        }
    }

    /// [`translate`] in STATEMENT position, which one shape is measured by:
    /// a `begin ... end` carries a `BEGIN` wrapper as an EXPRESSION
    /// (`x = begin 1 end` is `LASGN[:x, BEGIN[INTEGER]]`) and none as a
    /// statement. Prism's node is the same either way, so the position has to
    /// come from the caller -- and every statement context in this translator
    /// reaches its children through `statements_body`.
    fn translate_stmt(n: &P<'_>, cx: &mut Cx, in_block: bool) -> RubyValue {
        if let Some(x) = n.as_begin_node() {
            let loc = n.location();
            return begin_body(
                &x,
                cx,
                in_block,
                loc.start_offset(),
                loc.end_offset(),
                false,
            );
        }
        translate(n, cx, in_block)
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
    /// A body's statements, with the leading empty statement CRuby's grammar
    /// leaves ahead of an unabsorbed terminator -- see
    /// [`Cx::leading_terminator`], which decides whether there is one.
    ///
    /// `header_end` is where the construct's opening ends: past `begin`, past
    /// `class K`, past a block's `{`/`do` and its parameter list.
    fn body_after(
        stmts: Option<ruby_prism::StatementsNode<'_>>,
        header_end: usize,
        absorb: Absorb,
        cx: &mut Cx,
        b: bool,
    ) -> RubyValue {
        let gap = cx.leading_gap(header_end, absorb);
        let els: Vec<P<'_>> = stmts
            .as_ref()
            .map(|st| st.body().iter().collect())
            .unwrap_or_default();
        match (gap, els.is_empty()) {
            // Nothing but the terminator: the empty statement IS the body.
            (Some(at), true) => empty_begin(cx, at),
            (None, true) => RubyValue::Nil,
            (None, false) => statements_body(&stmts.expect("non-empty"), cx, b),
            (Some(at), false) => {
                let mut kids = vec![empty_begin(cx, at)];
                kids.extend(els.iter().map(|n| translate_stmt(n, cx, b)));
                let loc = stmts.expect("non-empty").location();
                node(cx, "BLOCK", at, loc.end_offset(), kids)
            }
        }
    }

    /// A `when`/`in` arm's body. An EMPTY one is CRuby's zero-width `BEGIN`
    /// holding nil, sited where the arm's header ends, not a bare nil.
    fn arm_body(
        stmts: Option<ruby_prism::StatementsNode<'_>>,
        at: usize,
        cx: &mut Cx,
        b: bool,
    ) -> RubyValue {
        match stmts {
            Some(st) if st.body().iter().next().is_some() => statements_body(&st, cx, b),
            _ => {
                let at = cx.through_terminator(at);
                empty_begin(cx, at)
            }
        }
    }

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
        let lo = conds
            .first()
            .map_or(loc.start_offset(), |c| c.location().start_offset());
        let hi = conds
            .last()
            .map_or(loc.end_offset(), |c| c.location().end_offset());
        let head = arg_list(&conds, lo, hi, cx, b).unwrap_or(RubyValue::Nil);
        let body = arm_body(w.statements(), loc.end_offset(), cx, b);
        let next = when_chain(arms, idx + 1, tail, cx, b);
        let (ws, we) = through(loc.start_offset(), loc.end_offset(), &next);
        let we = cx.through_terminator(we);
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
        e: usize,
        wrap: bool,
    ) -> RubyValue {
        // An empty body sits at the END of the keyword that opened it, and is
        // a `BEGIN` holding one nil rather than nothing.
        let body_at = x.begin_keyword_loc().map_or(s, |k| k.end_offset());
        let mut inner = match x.begin_keyword_loc() {
            Some(k) => body_after(x.statements(), k.end_offset(), Absorb::Newlines, cx, b),
            // A `def`/block body prism also models as a BeginNode when it
            // carries a rescue: there is no `begin` keyword, and the
            // terminator ahead of it belongs to the enclosing construct.
            None => match x.statements() {
                Some(stmts) if stmts.body().iter().next().is_some() => {
                    statements_body(&stmts, cx, b)
                }
                _ => empty_begin(cx, body_at),
            },
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
            let body = match body_after(en.statements(), at, Absorb::Newlines, cx, b) {
                RubyValue::Nil => empty_begin(cx, at),
                other => other,
            };
            let end = range_of(&body).map_or(start, |(_, en2)| en2);
            let end = cx.through_terminator(end);
            inner = node(cx, "ENSURE", start, end, vec![inner, body]);
        }
        // The expression-position wrapper, spanning `begin` through `end` --
        // see [`translate_stmt`].
        if wrap {
            inner = node(cx, "BEGIN", s, e, vec![inner]);
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
        // A `rescue` clause runs to its SEPARATOR, the same rule a `when`
        // arm follows -- see `Cx::through_terminator`.
        let re = cx.through_terminator(re);
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

    /// An interpolated literal, in CRuby's three-child shape -- see the call
    /// sites for what the three are.
    ///
    /// One span is not derivable from the parts and is copied: the first
    /// `EVSTR` takes the WHOLE literal's span when the literal is exactly one
    /// interpolation (`"#{b}"`), and its own when anything else is there
    /// (`"a#{b}"`, `"#{b}c"`).
    /// An interpolated literal's parts, with adjacent string literals
    /// FLATTENED: `"a" "#{b}"` is one literal in CRuby, and prism nests the
    /// second inside the first's parts.
    fn flat_parts(list: ruby_prism::NodeList<'_>) -> Vec<(P<'_>, Option<(usize, usize)>)> {
        let mut out = Vec::new();
        for p in list.iter() {
            let Some(inner) = p.as_interpolated_string_node() else {
                out.push((p, None));
                continue;
            };
            let loc = inner.location();
            let mut nested = flat_parts(inner.parts());
            // An inner literal that is exactly ONE interpolation lends it its
            // span, which the flattening would otherwise lose.
            if let [(only, span)] = nested.as_mut_slice()
                && only.as_string_node().is_none()
            {
                *span = Some((loc.start_offset(), loc.end_offset()));
            }
            out.extend(nested);
        }
        out
    }

    fn dstr(
        cx: &mut Cx,
        kind: &'static str,
        parts: &[(P<'_>, Option<(usize, usize)>)],
        s: usize,
        e: usize,
        b: bool,
    ) -> RubyValue {
        // The literal text before the first interpolation, if any.
        let (lead, rest) = match parts.first().and_then(|(p, _)| p.as_string_node()) {
            Some(str_node) => (
                String::from_utf8_lossy(str_node.unescaped()).into_owned(),
                &parts[1..],
            ),
            None => (String::new(), parts),
        };
        let lead = RubyValue::Str(crate::string_new(lead));
        let Some(((head, head_span), tail)) = rest.split_first() else {
            return node(cx, kind, s, e, vec![lead, RubyValue::Nil, RubyValue::Nil]);
        };
        // A literal that is exactly one interpolation lends it its span --
        // the whole node's when nothing was flattened, and the INNER
        // literal's when something was.
        let whole = match head_span {
            Some(sp) => Some(*sp),
            None => (tail.is_empty() && rest.len() == parts.len()).then_some((s, e)),
        };
        let first = dstr_part(cx, head, whole, b);
        let rest_list = match tail.split_first() {
            None => RubyValue::Nil,
            Some(((h, _), _)) => {
                let hl = h.location();
                let elems: Vec<RubyValue> = tail
                    .iter()
                    .map(|(p, sp)| dstr_part(cx, p, *sp, b))
                    .collect();
                list_node(cx, hl.start_offset(), hl.end_offset(), elems)
            }
        };
        node(cx, kind, s, e, vec![lead, first, rest_list])
    }

    /// One part of an interpolated literal: a literal run is `STR`, an
    /// interpolation is `EVSTR` over what it holds. `span` overrides the
    /// EVSTR's own -- see [`dstr`].
    fn dstr_part(cx: &mut Cx, p: &P<'_>, span: Option<(usize, usize)>, b: bool) -> RubyValue {
        let loc = p.location();
        let (ps, pe) = span.unwrap_or((loc.start_offset(), loc.end_offset()));
        if let Some(str_node) = p.as_string_node() {
            let v = RubyValue::Str(crate::string_new(
                String::from_utf8_lossy(str_node.unescaped()).into_owned(),
            ));
            return node(cx, "STR", ps, pe, vec![v]);
        }
        let inner = match p.as_embedded_statements_node() {
            // `#{}` holds CRuby's zero-width empty statement, sited just past
            // the opening delimiter.
            Some(es) => {
                let at = es.opening_loc().end_offset();
                match body_after(es.statements(), at, Absorb::Newlines, cx, b) {
                    RubyValue::Nil => empty_begin(cx, at),
                    other => other,
                }
            }
            None => match p.as_embedded_variable_node() {
                Some(ev) => translate(&ev.variable(), cx, b),
                None => translate(p, cx, b),
            },
        };
        node(cx, "EVSTR", ps, pe, vec![inner])
    }

    /// `LIST` -- CRuby's cons-shaped array node: elements then a trailing nil.
    fn list_node(cx: &mut Cx, start: usize, end: usize, mut elems: Vec<RubyValue>) -> RubyValue {
        elems.push(RubyValue::Nil);
        node(cx, "LIST", start, end, elems)
    }

    /// One element list built the way CRuby's `arg_append`/`arg_concat`
    /// (parse.y) build it, which is what every splat-carrying list is: a call's
    /// arguments, an array literal, a multiple assignment's right side, and a
    /// `when`'s conditions.
    ///
    /// The accumulator changes SHAPE as the splats land. A run of plain
    /// elements is a `LIST`; a splat concatenates onto it (`ARGSCAT`, whose
    /// body is the splatted VALUE, not a `SPLAT` node); a plain element after a
    /// splat pushes onto that (`ARGSPUSH`); and a SECOND plain element folds
    /// the push back into an `ARGSCAT` over a two-element `LIST` -- CRuby's
    /// `nd_set_type(node1, NODE_ARGSCAT)`. A list with no splat is the ordinary
    /// `LIST`, and a lone splat is a bare `SPLAT`.
    struct Arg {
        kind: ArgKind,
        lo: usize,
        hi: usize,
    }

    enum ArgKind {
        List(Vec<RubyValue>),
        /// A lone splat: its VALUE, wrapped in `SPLAT` when built.
        Splat(RubyValue),
        /// `ARGSCAT(head, value)` -- the splatted value verbatim.
        Cat(Box<Arg>, RubyValue),
        /// `ARGSCAT(head, LIST(elems))`, the fold an `ARGSPUSH` becomes.
        CatList(Box<Arg>, Vec<RubyValue>, usize, usize),
        /// `ARGSPUSH(head, tail)`, carrying the tail's own START for the fold
        /// -- the `LIST` it becomes begins there, and ends at whatever the
        /// element that triggers the fold ends at.
        Push(Box<Arg>, RubyValue, usize),
    }

    /// Materialize, with `start`/`end` overriding the OUTERMOST span: an array
    /// literal's brackets, a call's argument list. Everything underneath spans
    /// exactly the elements it covers.
    fn build_arg(a: Arg, cx: &mut Cx, start: usize, end: usize) -> RubyValue {
        let nested = |h: Box<Arg>, cx: &mut Cx| {
            let (l, r) = (h.lo, h.hi);
            build_arg(*h, cx, l, r)
        };
        match a.kind {
            ArgKind::List(elems) => list_node(cx, start, end, elems),
            ArgKind::Splat(v) => node(cx, "SPLAT", start, end, vec![v]),
            ArgKind::Cat(head, v) => {
                let h = nested(head, cx);
                node(cx, "ARGSCAT", start, end, vec![h, v])
            }
            ArgKind::CatList(head, elems, blo, bhi) => {
                let h = nested(head, cx);
                let list = list_node(cx, blo, bhi, elems);
                node(cx, "ARGSCAT", start, end, vec![h, list])
            }
            ArgKind::Push(head, tail, _) => {
                let h = nested(head, cx);
                node(cx, "ARGSPUSH", start, end, vec![h, tail])
            }
        }
    }

    fn arg_append(acc: Option<Arg>, tail: RubyValue, lo: usize, hi: usize) -> Arg {
        let Some(a) = acc else {
            return Arg {
                kind: ArgKind::List(vec![tail]),
                lo,
                hi,
            };
        };
        let l = a.lo;
        let kind = match a.kind {
            ArgKind::List(mut elems) => {
                elems.push(tail);
                ArgKind::List(elems)
            }
            ArgKind::Push(head, first, flo) => ArgKind::CatList(head, vec![first, tail], flo, hi),
            ArgKind::CatList(head, mut elems, blo, _) => {
                elems.push(tail);
                ArgKind::CatList(head, elems, blo, hi)
            }
            other => ArgKind::Push(
                Box::new(Arg {
                    kind: other,
                    lo: l,
                    hi: a.hi,
                }),
                tail,
                lo,
            ),
        };
        Arg { kind, lo: l, hi }
    }

    fn arg_concat(acc: Option<Arg>, value: RubyValue, lo: usize, hi: usize) -> Arg {
        match acc {
            // A lone splat stands alone, as a `SPLAT`.
            None => Arg {
                kind: ArgKind::Splat(value),
                lo,
                hi,
            },
            Some(a) => {
                let l = a.lo;
                Arg {
                    kind: ArgKind::Cat(Box::new(a), value),
                    lo: l,
                    hi,
                }
            }
        }
    }

    /// `break`/`next`/`return`'s argument, which is an ordinary element list
    /// except that `unwrap_single` kinds hand a LONE argument through bare
    /// (`return 1` is the INTEGER, `next 1` is a one-element LIST).
    fn jump_arg(
        args: Option<ruby_prism::ArgumentsNode<'_>>,
        unwrap_single: bool,
        cx: &mut Cx,
        b: bool,
    ) -> RubyValue {
        let Some(a) = args else {
            return RubyValue::Nil;
        };
        let els: Vec<P<'_>> = a.arguments().iter().collect();
        if unwrap_single && els.len() == 1 && els[0].as_splat_node().is_none() {
            return translate(&els[0], cx, b);
        }
        let al = a.location();
        arg_list(&els, al.start_offset(), al.end_offset(), cx, b).unwrap_or(RubyValue::Nil)
    }

    /// The shared driver: translate `els` left to right, splats through
    /// [`arg_concat`] and everything else through [`arg_append`]. `None` for an
    /// empty list, so a caller with nothing to say answers nil.
    fn arg_list(
        els: &[P<'_>],
        start: usize,
        end: usize,
        cx: &mut Cx,
        b: bool,
    ) -> Option<RubyValue> {
        let mut acc: Option<Arg> = None;
        for el in els {
            let loc = el.location();
            let (lo, hi) = (loc.start_offset(), loc.end_offset());
            acc = Some(match el.as_splat_node().and_then(|sp| sp.expression()) {
                Some(v) => {
                    let value = translate(&v, cx, b);
                    arg_concat(acc, value, lo, hi)
                }
                None => {
                    let v = translate(el, cx, b);
                    arg_append(acc, v, lo, hi)
                }
            });
        }
        acc.map(|a| build_arg(a, cx, start, end))
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
        // An INTERPOLATED literal: `DSTR [leading_text, first_part, rest]`,
        // where `rest` is a LIST of everything after the first part and the
        // leading text is a bare Ruby String -- `""` when the interpolation
        // comes first. `:"..."` is DSYM, `/.../` DREGX, a backtick command
        // DXSTR, all three the same three children.
        if let Some(x) = n.as_interpolated_string_node() {
            let parts = flat_parts(x.parts());
            return dstr(cx, "DSTR", &parts, s, e, in_block);
        }
        if let Some(x) = n.as_interpolated_symbol_node() {
            let parts = flat_parts(x.parts());
            return dstr(cx, "DSYM", &parts, s, e, in_block);
        }
        if let Some(x) = n.as_interpolated_regular_expression_node() {
            let parts = flat_parts(x.parts());
            return dstr(cx, "DREGX", &parts, s, e, in_block);
        }
        if let Some(x) = n.as_interpolated_x_string_node() {
            let parts = flat_parts(x.parts());
            return dstr(cx, "DXSTR", &parts, s, e, in_block);
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
        // `A::B` is COLON2 over its scope; a top-level `::A` is COLON3, which
        // holds the NAME alone -- there is nothing to its left.
        if let Some(x) = n.as_constant_path_node() {
            let name = match x.name() {
                Some(nm) => sym_val(nm.as_slice()),
                None => RubyValue::Nil,
            };
            return match x.parent() {
                Some(p) => {
                    let scope = translate(&p, cx, in_block);
                    node(cx, "COLON2", s, e, vec![scope, name])
                }
                None => node(cx, "COLON3", s, e, vec![name]),
            };
        }
        // `__FILE__` carries the script name, which for a string parse is
        // empty; `__LINE__` carries the line it sits on.
        if n.as_source_file_node().is_some() {
            let name = cx
                .script
                .as_ref()
                .map_or_else(String::new, |_| String::new());
            return node(
                cx,
                "FILE",
                s,
                e,
                vec![RubyValue::Str(crate::string_new(name))],
            );
        }
        if n.as_source_line_node().is_some() {
            let (line, _) = cx.pos(s);
            return node(cx, "LINE", s, e, vec![RubyValue::Int(line)]);
        }
        if n.as_source_encoding_node().is_some() {
            return node(cx, "ENCODING", s, e, vec![]);
        }
        if let Some(x) = n.as_constant_write_node() {
            let value = translate(&x.value(), cx, in_block);
            return node(cx, "CDECL", s, e, vec![sym_val(x.name().as_slice()), value]);
        }
        // `A::B = 1` -- a SCOPED write names its scope as well as its leaf,
        // so the CDECL carries three children where a bare one carries two.
        if let Some(x) = n.as_constant_path_write_node() {
            let target = x.target();
            let path = translate(&target.as_node(), cx, in_block);
            let name = match target.name() {
                Some(nm) => sym_val(nm.as_slice()),
                None => RubyValue::Nil,
            };
            let value = translate(&x.value(), cx, in_block);
            return node(cx, "CDECL", s, e, vec![path, name, value]);
        }
        if let Some(x) = n.as_array_node() {
            let els: Vec<P<'_>> = x.elements().iter().collect();
            return arg_list(&els, s, e, cx, in_block)
                .unwrap_or_else(|| list_node(cx, s, e, Vec::new()));
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
                // A block's parameter list takes the newline after it, never
                // a `;` -- so `do |z|; 1; end` carries the empty statement and
                // `do |z|\n1\nend` does not. `pe` is past the last parameter;
                // the closing `|` is the byte after it.
                let after_header = match &params {
                    Some(_) => pe + 1,
                    None => block.opening_loc().end_offset(),
                };
                let args = match &params {
                    Some(_) => args_node(params, ps, pe, cx, true),
                    None => RubyValue::Nil,
                };
                let locals: Vec<RubyValue> = block
                    .locals()
                    .iter()
                    .map(|l| sym_val(l.as_slice()))
                    .collect();
                let body = body_after(
                    block.body().and_then(|b| b.as_statements_node()),
                    after_header,
                    Absorb::Newlines,
                    cx,
                    true,
                );
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
            // Its argument list takes ONE terminator, so only a SECOND
            // separator (`def m; ; 1; end`) leaves the empty statement.
            let body = match x.body() {
                Some(b) => match b.as_statements_node() {
                    Some(stmts) => body_after(Some(stmts), pe, Absorb::One, cx, false),
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
            return begin_body(&x, cx, in_block, s, e, true);
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
            let arg = jump_arg(x.arguments(), true, cx, in_block);
            return node(cx, "BREAK", s, e, vec![arg]);
        }
        if let Some(x) = n.as_next_node() {
            let arg = jump_arg(x.arguments(), false, cx, in_block);
            return node(cx, "NEXT", s, e, vec![arg]);
        }
        if let Some(x) = n.as_return_node() {
            let arg = jump_arg(x.arguments(), true, cx, in_block);
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
            // A superclass clause takes the terminator after it; with no
            // clause nothing does, so even a newline leaves the empty
            // statement CRuby's `stmts: none` reduces to. An empty body is
            // that statement alone.
            let absorb = match x.superclass() {
                Some(_) => Absorb::One,
                None => Absorb::None,
            };
            let body = match x.body().and_then(|b| b.as_statements_node()) {
                Some(stmts) => body_after(Some(stmts), body_end, absorb, cx, false),
                None => body_after(None, body_end, absorb, cx, false),
            };
            let body = match body {
                RubyValue::Nil => node(cx, "BEGIN", body_end, body_end, vec![RubyValue::Nil]),
                other => other,
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
        // `class << expr` -- SCLASS over the receiver and a SCOPE. The `<<`
        // rule takes the terminator after the expression, so the body carries
        // no leading empty statement.
        if let Some(x) = n.as_singleton_class_node() {
            let recv = translate(&x.expression(), cx, in_block);
            let at = x.expression().location().end_offset();
            let body = body_after(
                x.body().and_then(|b| b.as_statements_node()),
                at,
                Absorb::One,
                cx,
                false,
            );
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
            return node(cx, "SCLASS", s, e, vec![recv, scope]);
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
            // A module body has no superclass clause to take the terminator
            // -- see the `class` arm.
            let at = cloc.end_offset();
            let body = match body_after(
                x.body().and_then(|b| b.as_statements_node()),
                at,
                Absorb::None,
                cx,
                false,
            ) {
                RubyValue::Nil => node(cx, "BEGIN", at, at, vec![RubyValue::Nil]),
                other => other,
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
        // A parenthesised statements list is a `BLOCK` in CRuby, spanning the
        // parens -- `x = (1)` is `LASGN[:x, BLOCK[INTEGER]]`.
        if let Some(x) = n.as_parentheses_node()
            && let Some(body) = x.body().and_then(|b| b.as_statements_node())
        {
            let at = x.opening_loc().end_offset();
            let inner = body_after(Some(body), at, Absorb::Newlines, cx, in_block);
            return node(cx, "BLOCK", s, e, vec![inner]);
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
        let args = x.arguments().and_then(|a| {
            let al = a.location();
            let els: Vec<P<'_>> = a.arguments().iter().collect();
            arg_list(&els, al.start_offset(), al.end_offset(), cx, in_block)
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
        let args = x.arguments().and_then(|a| {
            let al = a.location();
            let els: Vec<P<'_>> = a.arguments().iter().collect();
            arg_list(&els, al.start_offset(), al.end_offset(), cx, in_block)
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
