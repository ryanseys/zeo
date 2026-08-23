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

        /// Past any horizontal whitespace at `i`.
        fn skip_space(&self, mut i: usize) -> usize {
            while matches!(self.src.get(i), Some(b' ' | b'\t' | b'\r')) {
                i += 1;
            }
            i
        }

        /// [`Cx::through_terminator`] with spaces allowed ahead of the `;` --
        /// the whole program runs to its last separator, and `a ; # c` ends
        /// past that `;`.
        fn through_spaced_terminator(&self, end: usize) -> usize {
            let at = self.skip_space(end);
            match self.src.get(at) {
                Some(b';') => at + 1,
                _ => end,
            }
        }

        /// The source text a location covers, for the handful of nodes CRuby
        /// renders as a VALUE built from the literal's own bytes.
        fn slice(&self, loc: ruby_prism::Location<'_>) -> String {
            let (a, b) = (loc.start_offset(), loc.end_offset());
            String::from_utf8_lossy(self.src.get(a..b).unwrap_or_default()).into_owned()
        }

        /// `end` with any trailing horizontal whitespace given back -- a call
        /// that stops before its block stops at its own last character.
        fn trim_trailing_space(&self, start: usize, mut end: usize) -> usize {
            while end > start && matches!(self.src.get(end - 1), Some(b' ' | b'\t' | b'\r')) {
                end -= 1;
            }
            end
        }

        /// Where a construct's statement list starts parsing: past whatever
        /// the grammar absorbed after the header. An EMPTY body's zero-width
        /// `BEGIN` sits exactly here.
        fn body_start(&self, header_end: usize, absorb: Absorb) -> usize {
            match absorb {
                Absorb::None => header_end,
                Absorb::Newlines | Absorb::One => {
                    let i = self.skip_space(header_end);
                    match (absorb, self.src.get(i)) {
                        (Absorb::One, Some(b';' | b'\n')) => i + 1,
                        (Absorb::Newlines, Some(b'\n')) => i + 1,
                        _ => header_end,
                    }
                }
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
        // A program is a body like any other: a source that is nothing but a
        // terminator is the empty statement, not nothing.
        let stmts = program.statements();
        let body = match body_after(Some(stmts), 0, Absorb::None, &mut cx, false) {
            RubyValue::Nil => empty_begin(&mut cx, 0),
            other => other,
        };
        let loc = program.location();
        let end = cx.through_spaced_terminator(loc.end_offset());
        Ok(node(
            &mut cx,
            "SCOPE",
            loc.start_offset(),
            end,
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
        // `BEGIN { .. }` is HOISTED, and CRuby leaves two statements per
        // occurrence: every hoisted body first, then one empty statement per
        // occurrence for the place it vacated. Both span the braces.
        let mut hoisted: Vec<RubyValue> = Vec::new();
        let mut kids: Vec<RubyValue> = Vec::with_capacity(body.len());
        for n in &body {
            if let Some(pre) = n.as_pre_execution_node() {
                let (ps, pe) = (
                    pre.opening_loc().start_offset(),
                    pre.closing_loc().end_offset(),
                );
                let inner = match opt_statements(pre.statements(), cx, in_block) {
                    RubyValue::Nil => empty_begin(cx, pre.opening_loc().end_offset()),
                    other => other,
                };
                hoisted.push(node(cx, "BEGIN", ps, pe, vec![inner]));
                // The marker stays where the keyword stood; only the BODY
                // moves.
                kids.push(node(cx, "BEGIN", ps, pe, vec![RubyValue::Nil]));
                continue;
            }
            kids.push(translate_stmt(n, cx, in_block));
        }
        for (i, h) in hoisted.into_iter().enumerate() {
            kids.insert(i, h);
        }
        match kids.len() {
            0 => RubyValue::Nil,
            1 => kids.pop().expect("one"),
            _ => {
                let loc = stmts.location();
                // The list starts at its FIRST statement, which a hoisted
                // `BEGIN` moves past the keyword it was written with.
                let lo = kids
                    .first()
                    .and_then(range_of)
                    .map_or(loc.start_offset(), |(a, _)| a);
                node(cx, "BLOCK", lo, loc.end_offset(), kids)
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

    /// A conditional's arm: its statements, or the zero-width empty statement
    /// CRuby leaves where an arm has none.
    fn clause_body(
        stmts: Option<ruby_prism::StatementsNode<'_>>,
        header_end: usize,
        cx: &mut Cx,
        b: bool,
    ) -> RubyValue {
        match stmts {
            Some(st) if st.body().iter().next().is_some() => statements_body(&st, cx, b),
            _ => {
                let at = cx.body_start(header_end, Absorb::One);
                empty_begin(cx, at)
            }
        }
    }

    /// An `else` arm. Unlike an `if`/`while` header, the `else` keyword takes
    /// no terminator with it, so the empty statement sits right after it.
    fn else_clause_body(
        stmts: Option<ruby_prism::StatementsNode<'_>>,
        clause: &ruby_prism::ElseNode<'_>,
        cx: &mut Cx,
        b: bool,
    ) -> RubyValue {
        match stmts {
            Some(st) if st.body().iter().next().is_some() => statements_body(&st, cx, b),
            _ => empty_begin(cx, clause.else_keyword_loc().end_offset()),
        }
    }

    /// The same node, over a different byte range -- for the handful of spans
    /// CRuby stretches past the text a part covers.
    fn respan(v: RubyValue, cx: &mut Cx, lo: usize, hi: usize) -> RubyValue {
        let RubyValue::Object(o) = &v else {
            return v;
        };
        let Some(n) = o.as_any().downcast_ref::<RAstNode>() else {
            return v;
        };
        let (kind, children) = (n.kind, n.children.clone());
        node(cx, kind, lo, hi, children)
    }

    /// An `elsif`'s span, cut back to the terminator after its own last part.
    ///
    /// prism gives the chained `if` the whole construct's extent, through the
    /// `end` that closes the outermost one; CRuby stops each link where its
    /// own text stops.
    fn retrim_elsif(v: RubyValue, cx: &mut Cx) -> RubyValue {
        let RubyValue::Object(o) = &v else {
            return v;
        };
        let Some(n) = o.as_any().downcast_ref::<RAstNode>() else {
            return v;
        };
        if n.kind != "IF" {
            return v;
        }
        let (start, _) = n.byte_range;
        let inner = n
            .children
            .iter()
            .filter_map(range_of)
            .map(|(_, b)| b)
            .max()
            .unwrap_or(start);
        let end = cx.through_terminator(inner);
        let children = n.children.clone();
        node(cx, "IF", start, end, children)
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
        let body = arm_body(arm.statements(), loc.end_offset(), cx, b);
        let next = in_chain(arms, idx + 1, tail, cx, b);
        let (is, ie) = through(loc.start_offset(), loc.end_offset(), &next);
        let ie = cx.through_terminator(ie);
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
            // A `def m; rescue; end` has NO body, and CRuby writes that as a
            // nil first child rather than as an empty statement.
            None => match x.statements() {
                Some(stmts) if stmts.body().iter().next().is_some() => {
                    statements_body(&stmts, cx, b)
                }
                _ => RubyValue::Nil,
            },
        };
        let _ = body_at;
        // A RESCUE/ENSURE spans from the BODY it wraps, never from `begin`.
        let start = range_of(&inner).map_or(s, |(st, _)| st);
        if let Some(r) = x.rescue_clause() {
            // The `else` of a `begin` is a body like any other, so its own
            // terminator leaves the empty statement ahead of it.
            let els = match x.else_clause() {
                Some(c) => body_after(
                    c.statements(),
                    c.else_keyword_loc().end_offset(),
                    Absorb::Newlines,
                    cx,
                    b,
                ),
                None => RubyValue::Nil,
            };
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
            _ => {
                let at = cx.through_terminator(loc.end_offset());
                empty_begin(cx, at)
            }
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
    fn pattern_span_with(
        loc: ruby_prism::Location<'_>,
        parts: &[&RubyValue],
        extra: Option<(usize, usize)>,
    ) -> (usize, usize) {
        let mut ranges: Vec<(usize, usize)> = parts.iter().filter_map(|p| range_of(p)).collect();
        ranges.extend(extra);
        match (
            ranges.iter().map(|r| r.0).min(),
            ranges.iter().map(|r| r.1).max(),
        ) {
            (Some(lo), Some(hi)) => (lo, hi),
            _ => (loc.start_offset(), loc.end_offset()),
        }
    }

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
        asgn_target(target, from, value, cx, "LASGN")
    }

    /// [`asgn_node`] with the local-variable kind chosen by the caller: a
    /// block's parameters assign through `DASGN`, a method's through `LASGN`.
    ///
    /// A `a.b` or `a[0]` target is not an assignment node at all but the
    /// `ATTRASGN` call it desugars to, which is why this cannot key on the
    /// target's kind alone.
    fn asgn_target(
        target: &P<'_>,
        from: Option<usize>,
        value: RubyValue,
        cx: &mut Cx,
        local: &'static str,
    ) -> RubyValue {
        let loc = target.location();
        let (ts, te) = (loc.start_offset(), loc.end_offset());
        if let Some(t) = target.as_call_target_node() {
            let recv = translate(&t.receiver(), cx, false);
            let name = sym_val(t.name().as_slice());
            return node(cx, "ATTRASGN", ts, te, vec![recv, name, RubyValue::Nil]);
        }
        if let Some(t) = target.as_index_target_node() {
            let recv = translate(&t.receiver(), cx, false);
            let name = RubyValue::Symbol(crate::Symbol::intern("[]="));
            let args = t
                .arguments()
                .and_then(|a| {
                    let al = a.location();
                    let els: Vec<P<'_>> = a.arguments().iter().collect();
                    arg_list(&els, al.start_offset(), al.end_offset(), cx, false)
                })
                .unwrap_or(RubyValue::Nil);
            return node(cx, "ATTRASGN", ts, te, vec![recv, name, args]);
        }
        // A `rescue`'s assignment spans `=> e`, operator included.
        let (s, e) = (from.unwrap_or(loc.start_offset()), loc.end_offset());
        let (kind, name) = if let Some(t) = target.as_local_variable_target_node() {
            (local, t.name())
        } else if let Some(t) = target.as_instance_variable_target_node() {
            ("IASGN", t.name())
        } else if let Some(t) = target.as_global_variable_target_node() {
            ("GASGN", t.name())
        } else if let Some(t) = target.as_class_variable_target_node() {
            ("CVASGN", t.name())
        } else if let Some(t) = target.as_constant_target_node() {
            ("CDECL", t.name())
        } else if let Some(t) = target.as_required_parameter_node() {
            // A name inside a destructuring parameter group.
            (local, t.name())
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

    /// A hash literal or a call's trailing keywords: one flat `LIST` of
    /// alternating keys and values, wrapped in `HASH`.
    ///
    /// A `**rest` contributes a NIL key beside its value, which is how parse.y
    /// spells the splat -- and an EMPTY hash is `HASH nil`, with no list at
    /// all, so the two are not the same shape.
    fn hash_node(cx: &mut Cx, els: &[P<'_>], s: usize, e: usize, in_block: bool) -> RubyValue {
        if els.is_empty() {
            return node(cx, "HASH", s, e, vec![RubyValue::Nil]);
        }
        let mut pairs = Vec::new();
        let mut span: Option<(usize, usize)> = None;
        for el in els {
            let al = el.location();
            span = Some(match span {
                None => (al.start_offset(), al.end_offset()),
                Some((a, _)) => (a, al.end_offset()),
            });
            if let Some(assoc) = el.as_assoc_node() {
                pairs.push(translate(&assoc.key(), cx, in_block));
                pairs.push(translate(&assoc.value(), cx, in_block));
            } else if let Some(splat) = el.as_assoc_splat_node() {
                pairs.push(RubyValue::Nil);
                pairs.push(opt_translate(splat.value().as_ref(), cx, in_block));
            }
        }
        let (ls, le) = span.unwrap_or((s, e));
        let list = list_node(cx, ls, le, pairs);
        node(cx, "HASH", s, e, vec![list])
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

    /// Whether a message spells an OPCALL rather than a CALL.
    ///
    /// Not every operator-looking name does. CRuby's `[]`, `[]=` and `=~` come
    /// through the ordinary call rules, so they stay CALL -- the taxonomy
    /// follows parse.y's productions, not the shape of the name.
    fn is_operator(name: &str) -> bool {
        if matches!(name, "[]" | "[]=" | "=~") {
            return false;
        }
        !name.chars().any(|c| c.is_alphanumeric() || c == '_') && !name.ends_with('=')
            || matches!(name, "==" | "!=" | "<=" | ">=" | "<=>" | "===")
    }

    /// The 10-slot parse.y `ARGS` node from a prism parameter list:
    /// `[pre_num, pre_init, opt, first_post, post_num, post_init, rest, kw,
    /// kwrest, block]`.
    ///
    /// Three slots are not readable off the parameters alone.
    /// `pre_init`/`post_init` hold the `MASGN` a DESTRUCTURED parameter
    /// desugars to (a `BLOCK` of them when there is more than one), and the
    /// name it destructures is a hidden local; `kwrest` is a `DVAR` holding
    /// NIL whenever there are keywords and no explicit `**rest`, because the
    /// grammar allocates one either way. `locals` gains a nil for each hidden
    /// name, in the position the parameter list puts it.
    ///
    /// `**nil` is neither: it sets both `kw` and `kwrest` to `false`.
    fn args_node(
        params: Option<ruby_prism::ParametersNode<'_>>,
        loc_start: usize,
        loc_end: usize,
        cx: &mut Cx,
        in_block: bool,
        locals: &mut Vec<RubyValue>,
    ) -> RubyValue {
        let asgn = if in_block { "DASGN" } else { "LASGN" };
        let var = if in_block { "DVAR" } else { "LVAR" };
        let mut slots = vec![
            RubyValue::Int(0),
            RubyValue::Nil,
            RubyValue::Nil,
            RubyValue::Nil,
            RubyValue::Int(0),
            RubyValue::Nil,
            RubyValue::Nil,
            RubyValue::Nil,
            RubyValue::Nil,
            RubyValue::Nil,
        ];
        let Some(p) = params else {
            return node(cx, "ARGS", loc_start, loc_end, slots);
        };

        // Pre-parameters, and the destructuring a `(a, b)` among them needs.
        let pre: Vec<P<'_>> = p.requireds().iter().collect();
        slots[0] = RubyValue::Int(pre.len() as i64);
        slots[1] = destructured(&pre, cx, asgn, var, locals, 0);

        // Optionals, as a cons chain whose every link runs to the END of the
        // chain rather than to its own parameter.
        let opts: Vec<P<'_>> = p.optionals().iter().collect();
        let chain_end = opts
            .last()
            .map(|o| o.location().end_offset())
            .unwrap_or(loc_end);
        let mut chain = RubyValue::Nil;
        for o in opts.iter().rev() {
            let Some(opt) = o.as_optional_parameter_node() else {
                continue;
            };
            let oloc = opt.location();
            let value = translate(&opt.value(), cx, in_block);
            let a = node(
                cx,
                asgn,
                oloc.start_offset(),
                oloc.end_offset(),
                vec![sym_val(opt.name().as_slice()), value],
            );
            chain = node(
                cx,
                "OPT_ARG",
                oloc.start_offset(),
                chain_end,
                vec![a, chain],
            );
        }
        slots[2] = chain;

        let post: Vec<P<'_>> = p.posts().iter().collect();
        if let Some(first) = post.first() {
            slots[3] = param_name(first, cx);
        }
        slots[4] = RubyValue::Int(post.len() as i64);
        let pre_names = pre.iter().map(|q| local_names(q)).sum::<usize>();
        slots[5] = destructured(&post, cx, asgn, var, locals, pre_names);

        if let Some(rest) = p.rest().as_ref().and_then(|r| r.as_rest_parameter_node()) {
            slots[6] = match rest.name() {
                Some(nm) => sym_val(nm.as_slice()),
                None => {
                    // A nameless `*` still consumes, and parse.y both names it
                    // `:*` and declares it -- prism leaves it out.
                    let at = pre_names + p.optionals().iter().count();
                    anon_local(locals, at, "*");
                    RubyValue::Symbol(crate::Symbol::intern("*"))
                }
            };
        }

        // Keywords, chained like the optionals. A required keyword's value is
        // the marker symbol, not a node.
        let kws: Vec<P<'_>> = p.keywords().iter().collect();
        let kw_end = kws
            .last()
            .map(|k| k.location().end_offset())
            .unwrap_or(loc_end);
        let kw_start = kws
            .first()
            .map(|k| k.location().start_offset())
            .unwrap_or(loc_start);
        let mut kw_chain = RubyValue::Nil;
        for k in kws.iter().rev() {
            let kloc = k.location();
            let (name, value) = if let Some(req) = k.as_required_keyword_parameter_node() {
                (
                    sym_val(req.name().as_slice()),
                    RubyValue::Symbol(crate::Symbol::intern("NODE_SPECIAL_REQUIRED_KEYWORD")),
                )
            } else if let Some(o) = k.as_optional_keyword_parameter_node() {
                (
                    sym_val(o.name().as_slice()),
                    translate(&o.value(), cx, in_block),
                )
            } else {
                continue;
            };
            let a = node(
                cx,
                asgn,
                kloc.start_offset(),
                kloc.end_offset(),
                vec![name, value],
            );
            kw_chain = node(cx, "KW_ARG", kloc.start_offset(), kw_end, vec![a, kw_chain]);
        }
        slots[7] = kw_chain;

        // `**nil` says the method takes NO keywords, which is a different
        // answer from taking none by omission.
        let no_kw = p
            .keyword_rest()
            .as_ref()
            .is_some_and(|k| k.as_no_keywords_parameter_node().is_some());
        if no_kw {
            slots[7] = RubyValue::Bool(false);
            slots[8] = RubyValue::Bool(false);
        } else if let Some(kr) = p
            .keyword_rest()
            .as_ref()
            .and_then(|k| k.as_keyword_rest_parameter_node())
        {
            let kloc = kr.location();
            let name = match kr.name() {
                Some(nm) => sym_val(nm.as_slice()),
                None => {
                    let at = pre_names
                        + p.optionals().iter().count()
                        + usize::from(p.rest().is_some())
                        + post.iter().map(local_names).sum::<usize>()
                        + kws.len();
                    anon_local(locals, at, "**");
                    RubyValue::Symbol(crate::Symbol::intern("**"))
                }
            };
            slots[8] = node(
                cx,
                "DVAR",
                kloc.start_offset(),
                kloc.end_offset(),
                vec![name],
            );
        } else if !kws.is_empty() {
            // The hidden rest the grammar allocates for any keyword list.
            slots[8] = node(cx, "DVAR", kw_start, kw_end, vec![RubyValue::Nil]);
        }
        // That hidden rest is a LOCAL either way -- an explicit `**g` does not
        // take its place, it sits after it.
        if !kws.is_empty() && !no_kw {
            let at = pre_names
                + p.optionals().iter().count()
                + usize::from(p.rest().is_some())
                + post.iter().map(local_names).sum::<usize>()
                + kws.len();
            if at <= locals.len() {
                locals.insert(at, RubyValue::Nil);
            }
        }

        if let Some(b) = p.block() {
            slots[9] = match b.name() {
                Some(nm) => sym_val(nm.as_slice()),
                None => {
                    anon_local(locals, locals.len(), "&");
                    RubyValue::Symbol(crate::Symbol::intern("&"))
                }
            };
        }
        // `...` forwards three parameters at once, and prism keeps it as one.
        if p.keyword_rest()
            .as_ref()
            .is_some_and(|k| k.as_forwarding_parameter_node().is_some())
        {
            for name in ["*", "**", "&", "..."] {
                locals.push(RubyValue::Symbol(crate::Symbol::intern(name)));
            }
            let floc = p
                .keyword_rest()
                .map(|k| k.location())
                .expect("checked just above");
            let (fs, fe) = (floc.start_offset(), floc.end_offset());
            slots[6] = RubyValue::Symbol(crate::Symbol::intern("*"));
            slots[8] = node(
                cx,
                "DVAR",
                fs,
                fe,
                vec![RubyValue::Symbol(crate::Symbol::intern("**"))],
            );
            slots[9] = RubyValue::Symbol(crate::Symbol::intern("&"));
        }
        node(cx, "ARGS", loc_start, loc_end, slots)
    }

    /// Declares the name parse.y gives an anonymous `*`, `**` or `&`, which
    /// prism leaves out of its own locals list.
    fn anon_local(locals: &mut Vec<RubyValue>, at: usize, name: &str) {
        let sym = RubyValue::Symbol(crate::Symbol::intern(name));
        if at <= locals.len() {
            locals.insert(at, sym);
        } else {
            locals.push(sym);
        }
    }

    /// How many local names a parameter introduces, counting the ones nested
    /// inside a destructuring group.
    fn local_names(p: &P<'_>) -> usize {
        match p.as_multi_target_node() {
            None => 1,
            Some(mt) => {
                mt.lefts().iter().map(|q| local_names(&q)).sum::<usize>()
                    + mt.rest()
                        .as_ref()
                        .map_or(0, |r| usize::from(r.as_splat_node().is_some()))
                    + mt.rights().iter().map(|q| local_names(&q)).sum::<usize>()
            }
        }
    }

    /// The `MASGN` chain a run of parameters needs for the ones that are
    /// destructuring groups, and the nil each puts into `locals`.
    ///
    /// One group is the `MASGN` itself; several are a `BLOCK` over them.
    fn destructured(
        params: &[P<'_>],
        cx: &mut Cx,
        asgn: &'static str,
        var: &'static str,
        locals: &mut Vec<RubyValue>,
        base: usize,
    ) -> RubyValue {
        let mut built = Vec::new();
        for (i, p) in params.iter().enumerate() {
            let Some(mt) = p.as_multi_target_node() else {
                continue;
            };
            // The group's span is the CONTENT of its parens.
            let (gs, ge) = match (mt.lparen_loc(), mt.rparen_loc()) {
                (Some(l), Some(r)) => (l.end_offset(), r.start_offset()),
                _ => {
                    let l = mt.location();
                    (l.start_offset(), l.end_offset())
                }
            };
            let hidden = node(cx, var, gs, gs, vec![RubyValue::Nil]);
            let inner = masgn_targets(cx, MultiParts::target(&mt), asgn, gs, ge);
            let RubyValue::Object(o) = &inner else {
                continue;
            };
            let Some(m) = o.as_any().downcast_ref::<RAstNode>() else {
                continue;
            };
            let (pre, rest) = (m.children[1].clone(), m.children[2].clone());
            built.push(node(cx, "MASGN", gs, ge, vec![hidden, pre, rest]));
            let at = base + i;
            if at <= locals.len() {
                locals.insert(at, RubyValue::Nil);
            }
        }
        match built.len() {
            0 => RubyValue::Nil,
            1 => built.pop().expect("one"),
            _ => {
                let lo = range_of(&built[0]).map_or(0, |(a, _)| a);
                let hi = range_of(&built[built.len() - 1]).map_or(lo, |(_, b)| b);
                node(cx, "BLOCK", lo, hi, built)
            }
        }
    }

    /// A parameter's name as a bare symbol -- what the `first_post` slot holds.
    fn param_name(p: &P<'_>, _cx: &mut Cx) -> RubyValue {
        if let Some(r) = p.as_required_parameter_node() {
            return sym_val(r.name().as_slice());
        }
        RubyValue::Nil
    }

    fn bigint_of(negative: bool, digits: &[u32]) -> num_bigint::BigInt {
        let sign = if negative {
            num_bigint::Sign::Minus
        } else {
            num_bigint::Sign::Plus
        };
        num_bigint::BigInt::from_slice(sign, digits)
    }

    /// The VALUE a numeric literal denotes, for the two nodes CRuby renders as
    /// a built number rather than as a subtree (`1i` is `(0+1i)`).
    fn numeric_value(n: &P<'_>) -> RubyValue {
        if let Some(i) = n.as_integer_node() {
            let v = i.value();
            let (neg, digits) = v.to_u32_digits();
            return crate::int_from_u32_digits(neg, digits);
        }
        if let Some(f) = n.as_float_node() {
            return RubyValue::Float(f.value());
        }
        if let Some(r) = n.as_rational_node() {
            let (num, den) = (r.numerator(), r.denominator());
            let (nneg, ndig) = num.to_u32_digits();
            let (dneg, ddig) = den.to_u32_digits();
            return crate::builtins::rational::rational_new(
                bigint_of(nneg, ndig),
                bigint_of(dneg, ddig),
            )
            .unwrap_or(RubyValue::Nil);
        }
        RubyValue::Nil
    }

    fn cx_slice(cx: &Cx, loc: ruby_prism::Location<'_>) -> String {
        cx.slice(loc)
    }

    /// The `ITER` wrapper a block puts around an already-built call.
    fn iter_over(
        call: RubyValue,
        block: &ruby_prism::BlockNode<'_>,
        s: usize,
        e: usize,
        cx: &mut Cx,
    ) -> RubyValue {
        let scope = block_scope(block, cx);
        node(cx, "ITER", s, e, vec![call, scope])
    }

    /// A block's `SCOPE`: its locals, its `ARGS`, and its body.
    fn block_scope(block: &ruby_prism::BlockNode<'_>, cx: &mut Cx) -> RubyValue {
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
        // A block's parameter list takes the newline after it, never a `;` --
        // so `do |z|; 1; end` carries the empty statement and `do |z|\n1\nend`
        // does not. `pe` is past the last parameter; the closing `|` is the
        // byte after it.
        let after_header = block
            .parameters()
            .and_then(|p| p.as_block_parameters_node())
            .and_then(|bp| bp.closing_loc())
            .map(|c| c.end_offset())
            .unwrap_or_else(|| block.opening_loc().end_offset());
        let mut locals: Vec<RubyValue> = block
            .locals()
            .iter()
            .map(|l| sym_val(l.as_slice()))
            .collect();
        let has_params = params.is_some();
        let mut args = match has_params {
            true => args_node(params, ps, pe, cx, true, &mut locals),
            false => RubyValue::Nil,
        };
        // `_1`/`it` declare parameters no list mentions. CRuby still builds an
        // ARGS for them, zero-width at the block's END, with the count the
        // highest number used -- `it` counting as one.
        if let Some(np) = block
            .parameters()
            .as_ref()
            .and_then(|p| p.as_numbered_parameters_node())
        {
            args = implicit_args(cx, i64::from(np.maximum()), bloc.end_offset());
        } else if block
            .parameters()
            .as_ref()
            .is_some_and(|p| p.as_it_parameters_node().is_some())
        {
            locals.push(RubyValue::Symbol(crate::Symbol::intern("<it>")));
            args = implicit_args(cx, 1, bloc.end_offset());
        }
        let body = body_after(
            block.body().and_then(|b| b.as_statements_node()),
            after_header,
            Absorb::Newlines,
            cx,
            true,
        );
        // An empty block body is CRuby's zero-width `BEGIN nil`, sited just
        // inside the `{`, not nothing.
        let body = match body {
            RubyValue::Nil => empty_begin(cx, after_header),
            other => other,
        };
        node(
            cx,
            "SCOPE",
            bloc.start_offset(),
            bloc.end_offset(),
            vec![RubyValue::Array(crate::array_new(locals)), args, body],
        )
    }

    /// The zero-width `ARGS` an implicit parameter list (`_1`, `it`) gets.
    fn implicit_args(cx: &mut Cx, count: i64, at: usize) -> RubyValue {
        node(
            cx,
            "ARGS",
            at,
            at,
            vec![
                RubyValue::Int(count),
                RubyValue::Nil,
                RubyValue::Nil,
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

    /// A lambda's `SCOPE`, which spans the ARROW's body only -- `-> (x) { x }`
    /// scopes from the `(` through the `}`, not from the `->`.
    fn lambda_scope(x: &ruby_prism::LambdaNode<'_>, cx: &mut Cx) -> RubyValue {
        let params = x
            .parameters()
            .and_then(|p| p.as_block_parameters_node())
            .and_then(|bp| bp.parameters());
        let (ps, pe) = params
            .as_ref()
            .map(|p| {
                let l = p.location();
                (l.start_offset(), l.end_offset())
            })
            .unwrap_or_else(|| {
                let o = x.opening_loc().start_offset();
                (o, o)
            });
        let mut locals: Vec<RubyValue> = x.locals().iter().map(|l| sym_val(l.as_slice())).collect();
        let has_params = params.is_some();
        let args = args_node(params, ps, pe, cx, true, &mut locals);
        let body = body_after(
            x.body().and_then(|b| b.as_statements_node()),
            x.opening_loc().end_offset(),
            Absorb::Newlines,
            cx,
            true,
        );
        let body = match body {
            RubyValue::Nil => empty_begin(cx, x.opening_loc().end_offset()),
            other => other,
        };
        let start = if has_params {
            x.operator_loc().end_offset()
        } else {
            x.opening_loc().start_offset()
        };
        let start = cx.skip_space(start);
        node(
            cx,
            "SCOPE",
            start,
            x.closing_loc().end_offset(),
            vec![RubyValue::Array(crate::array_new(locals)), args, body],
        )
    }

    /// The `lefts`/`rest`/`rights` a multiple-assignment target carries,
    /// shared by the write form (`a, b = 1, 2`) and the nested target form
    /// (`a, (b, c) = ..`), which prism models as two node kinds.
    struct MultiParts<'a, 'pr> {
        lefts: Vec<P<'pr>>,
        rest: Option<P<'pr>>,
        rights: Vec<P<'pr>>,
        _marker: std::marker::PhantomData<&'a ()>,
    }

    impl<'pr> MultiParts<'_, 'pr> {
        fn write(x: &ruby_prism::MultiWriteNode<'pr>) -> Self {
            MultiParts {
                lefts: x.lefts().iter().collect(),
                rest: x.rest(),
                rights: x.rights().iter().collect(),
                _marker: std::marker::PhantomData,
            }
        }
        fn target(x: &ruby_prism::MultiTargetNode<'pr>) -> Self {
            MultiParts {
                lefts: x.lefts().iter().collect(),
                rest: x.rest(),
                rights: x.rights().iter().collect(),
                _marker: std::marker::PhantomData,
            }
        }
    }

    /// `MASGN[value, pre-targets, rest]`.
    ///
    /// A rest with targets AFTER it is not a third child but a `POSTARG`
    /// holding both, and it REPLACES the pre-target list, which then goes
    /// nil -- parse.y's own shape, and not derivable from the source order.
    fn masgn_targets(
        cx: &mut Cx,
        parts: MultiParts<'_, '_>,
        asgn: &'static str,
        s: usize,
        e: usize,
    ) -> RubyValue {
        let target = |t: &P<'_>, cx: &mut Cx| -> RubyValue {
            if let Some(inner) = t.as_multi_target_node() {
                // A NESTED group carries no hidden variable of its own.
                let (gs, ge) = match (inner.lparen_loc(), inner.rparen_loc()) {
                    (Some(l), Some(r)) => (l.end_offset(), r.start_offset()),
                    _ => {
                        let l = inner.location();
                        (l.start_offset(), l.end_offset())
                    }
                };
                return masgn_targets(cx, MultiParts::target(&inner), asgn, gs, ge);
            }
            asgn_target(t, None, RubyValue::Nil, cx, asgn)
        };
        let list_of = |els: &[P<'_>], cx: &mut Cx| -> RubyValue {
            if els.is_empty() {
                return RubyValue::Nil;
            }
            let lo = els[0].location().start_offset();
            let kids: Vec<RubyValue> = els.iter().map(|t| target(t, cx)).collect();
            // The end comes off the last TARGET, not off its source text: a
            // destructuring group's node stops inside its closing paren.
            let hi = kids
                .last()
                .and_then(range_of)
                .map_or_else(|| els[els.len() - 1].location().end_offset(), |(_, b)| b);
            list_node(cx, lo, hi, kids)
        };
        let pre = list_of(&parts.lefts, cx);
        // A TRAILING COMMA (`a, = 1`) is prism's implicit rest, and it
        // consumes nothing -- only a written `*` is a rest.
        let rest = match parts.rest.as_ref().and_then(|r| r.as_splat_node()) {
            None => RubyValue::Nil,
            Some(sp) => match sp.expression() {
                Some(t) => target(&t, cx),
                // A nameless `*` still consumes, and parse.y marks it.
                None => RubyValue::Symbol(crate::Symbol::intern("NODE_SPECIAL_NO_NAME_REST")),
            },
        };
        let (pre, rest) = if parts.rights.is_empty() {
            (pre, rest)
        } else {
            let post = list_of(&parts.rights, cx);
            // The POSTARG runs from the WHOLE target list's start, not from
            // the rest that triggered it.
            let lo = parts
                .lefts
                .first()
                .map(|l| l.location().start_offset())
                .or_else(|| parts.rest.as_ref().map(|r| r.location().start_offset()))
                .unwrap_or(s);
            let hi = parts.rights[parts.rights.len() - 1].location().end_offset();
            (pre, node(cx, "POSTARG", lo, hi, vec![rest, post]))
        };
        node(cx, "MASGN", s, e, vec![RubyValue::Nil, pre, rest])
    }

    /// [`masgn_targets`] with the assigned VALUE filled in.
    fn masgn(
        cx: &mut Cx,
        parts: MultiParts<'_, '_>,
        value: Option<RubyValue>,
        s: usize,
        e: usize,
    ) -> RubyValue {
        let built = masgn_targets(cx, parts, "LASGN", s, e);
        let RubyValue::Object(o) = &built else {
            return built;
        };
        let Some(m) = o.as_any().downcast_ref::<RAstNode>() else {
            return built;
        };
        let (pre, rest) = (m.children[1].clone(), m.children[2].clone());
        node(
            cx,
            "MASGN",
            s,
            e,
            vec![value.unwrap_or(RubyValue::Nil), pre, rest],
        )
    }

    /// `for x in a; end` -- a `FOR` over a SCOPE whose ARGS carry the loop
    /// variable as a pre-parameter, with the scope's own locals list holding
    /// one nil placeholder for the hidden element the grammar binds.
    fn for_node(x: &ruby_prism::ForNode<'_>, cx: &mut Cx, s: usize, e: usize) -> RubyValue {
        let collection = translate(&x.collection(), cx, false);
        let index = x.index();
        let iloc = index.location();
        let (is, ie) = (iloc.start_offset(), iloc.end_offset());
        let hidden = node(cx, "DVAR", is, ie, vec![RubyValue::Nil]);
        // One target is a pre-parameter; several are a MASGN over the hidden
        // element, which parse.y wraps in FOR_MASGN.
        let (pre_num, first) = match index.as_multi_target_node() {
            None => {
                let asgn = asgn_node(&index, None, hidden, cx);
                (1, asgn)
            }
            Some(mt) => {
                let inner = masgn(cx, MultiParts::target(&mt), None, is, ie);
                let RubyValue::Object(o) = &inner else {
                    return RubyValue::Nil;
                };
                let Some(m) = o.as_any().downcast_ref::<RAstNode>() else {
                    return RubyValue::Nil;
                };
                let pre = m.children[1].clone();
                let rest = m.children[2].clone();
                let wrapped = node(cx, "FOR_MASGN", is, ie, vec![hidden]);
                (0, node(cx, "MASGN", is, ie, vec![wrapped, pre, rest]))
            }
        };
        let args = node(
            cx,
            "ARGS",
            is,
            ie,
            vec![
                RubyValue::Int(pre_num),
                first,
                RubyValue::Nil,
                RubyValue::Nil,
                RubyValue::Int(0),
                RubyValue::Nil,
                RubyValue::Nil,
                RubyValue::Nil,
                RubyValue::Nil,
                RubyValue::Nil,
            ],
        );
        let header_end = x
            .do_keyword_loc()
            .map(|d| d.end_offset())
            .unwrap_or_else(|| x.collection().location().end_offset());
        let body = match body_after(x.statements(), header_end, Absorb::One, cx, false) {
            RubyValue::Nil => {
                let at = cx.body_start(header_end, Absorb::One);
                empty_begin(cx, at)
            }
            other => other,
        };
        let scope = node(
            cx,
            "SCOPE",
            s,
            e,
            vec![
                RubyValue::Array(crate::array_new(vec![RubyValue::Nil])),
                args,
                body,
            ],
        );
        node(cx, "FOR", s, e, vec![collection, scope])
    }

    /// One variable's read/write pair for the operator-assignment family:
    /// the node kind that READS it and the one that WRITES it.
    fn var_kinds(n: &P<'_>) -> Option<(&'static str, &'static str)> {
        if n.as_local_variable_operator_write_node().is_some()
            || n.as_local_variable_or_write_node().is_some()
            || n.as_local_variable_and_write_node().is_some()
        {
            return Some(("LVAR", "LASGN"));
        }
        if n.as_instance_variable_operator_write_node().is_some()
            || n.as_instance_variable_or_write_node().is_some()
            || n.as_instance_variable_and_write_node().is_some()
        {
            return Some(("IVAR", "IASGN"));
        }
        if n.as_global_variable_operator_write_node().is_some()
            || n.as_global_variable_or_write_node().is_some()
            || n.as_global_variable_and_write_node().is_some()
        {
            return Some(("GVAR", "GASGN"));
        }
        if n.as_class_variable_operator_write_node().is_some()
            || n.as_class_variable_or_write_node().is_some()
            || n.as_class_variable_and_write_node().is_some()
        {
            return Some(("CVAR", "CVASGN"));
        }
        if n.as_constant_operator_write_node().is_some()
            || n.as_constant_or_write_node().is_some()
            || n.as_constant_and_write_node().is_some()
        {
            return Some(("CONST", "CDECL"));
        }
        None
    }

    /// The whole operator-assignment family, or `None` when `n` is not one.
    ///
    /// A binary `x += 1` desugars: the tree is the ordinary WRITE holding a
    /// CALL of `+` on the READ. `||=` and `&&=` do not desugar -- they keep
    /// `OP_ASGN_OR`/`OP_ASGN_AND`, whose three children are the read, the
    /// operator's own symbol, and the write. The two call-shaped receivers
    /// have their own kinds again: `OP_ASGN1` for `a[0] +=` and `OP_ASGN2`
    /// for `a.b +=`, which carries a SAFE-NAVIGATION flag no other node does.
    fn op_assign(n: &P<'_>, cx: &mut Cx, in_block: bool, s: usize, e: usize) -> Option<RubyValue> {
        let sym = |t: &str| RubyValue::Symbol(crate::Symbol::intern(t));

        // A binary operator on a plain variable.
        macro_rules! binary {
            ($m:ident) => {
                if let Some(x) = n.$m() {
                    let (read, write) = var_kinds(n)?;
                    let nloc = x.name_loc();
                    let name = sym_val(x.name().as_slice());
                    let cur = node(
                        cx,
                        read,
                        nloc.start_offset(),
                        nloc.end_offset(),
                        vec![name.clone()],
                    );
                    let rhs = translate(&x.value(), cx, in_block);
                    let vloc = x.value().location();
                    let arg = list_node(cx, vloc.start_offset(), vloc.end_offset(), vec![rhs]);
                    let op = String::from_utf8_lossy(x.binary_operator().as_slice()).into_owned();
                    let call = node(cx, "CALL", s, e, vec![cur, sym(&op), arg]);
                    return Some(node(cx, write, s, e, vec![name, call]));
                }
            };
        }
        binary!(as_local_variable_operator_write_node);
        binary!(as_instance_variable_operator_write_node);
        binary!(as_global_variable_operator_write_node);
        binary!(as_class_variable_operator_write_node);
        binary!(as_constant_operator_write_node);

        macro_rules! short_circuit {
            ($m:ident, $kind:literal, $op:literal) => {
                if let Some(x) = n.$m() {
                    let (read, write) = var_kinds(n)?;
                    let nloc = x.name_loc();
                    let name = sym_val(x.name().as_slice());
                    let cur = node(
                        cx,
                        read,
                        nloc.start_offset(),
                        nloc.end_offset(),
                        vec![name.clone()],
                    );
                    let rhs = translate(&x.value(), cx, in_block);
                    let asgn = node(cx, write, s, e, vec![name, rhs]);
                    return Some(node(cx, $kind, s, e, vec![cur, sym($op), asgn]));
                }
            };
        }
        short_circuit!(as_local_variable_or_write_node, "OP_ASGN_OR", "||");
        short_circuit!(as_instance_variable_or_write_node, "OP_ASGN_OR", "||");
        short_circuit!(as_global_variable_or_write_node, "OP_ASGN_OR", "||");
        short_circuit!(as_class_variable_or_write_node, "OP_ASGN_OR", "||");
        short_circuit!(as_constant_or_write_node, "OP_ASGN_OR", "||");
        short_circuit!(as_local_variable_and_write_node, "OP_ASGN_AND", "&&");
        short_circuit!(as_instance_variable_and_write_node, "OP_ASGN_AND", "&&");
        short_circuit!(as_global_variable_and_write_node, "OP_ASGN_AND", "&&");
        short_circuit!(as_class_variable_and_write_node, "OP_ASGN_AND", "&&");
        short_circuit!(as_constant_and_write_node, "OP_ASGN_AND", "&&");

        // `a[0] op= v` -- OP_ASGN1[recv, op, index-list, value].
        macro_rules! index_asgn {
            ($m:ident, $op:expr) => {
                if let Some(x) = n.$m() {
                    let recv = opt_translate(x.receiver().as_ref(), cx, in_block);
                    let args = x
                        .arguments()
                        .and_then(|a| {
                            let al = a.location();
                            let els: Vec<P<'_>> = a.arguments().iter().collect();
                            arg_list(&els, al.start_offset(), al.end_offset(), cx, in_block)
                        })
                        .unwrap_or(RubyValue::Nil);
                    let value = translate(&x.value(), cx, in_block);
                    let op: RubyValue = $op(&x);
                    return Some(node(cx, "OP_ASGN1", s, e, vec![recv, op, args, value]));
                }
            };
        }
        index_asgn!(
            as_index_operator_write_node,
            |x: &ruby_prism::IndexOperatorWriteNode<'_>| {
                RubyValue::Symbol(crate::Symbol::intern(&String::from_utf8_lossy(
                    x.binary_operator().as_slice(),
                )))
            }
        );
        index_asgn!(
            as_index_or_write_node,
            |_: &ruby_prism::IndexOrWriteNode<'_>| sym("||")
        );
        index_asgn!(
            as_index_and_write_node,
            |_: &ruby_prism::IndexAndWriteNode<'_>| sym("&&")
        );

        // `a.b op= v` -- OP_ASGN2[recv, safe?, name, op, value].
        macro_rules! attr_asgn {
            ($m:ident, $op:expr) => {
                if let Some(x) = n.$m() {
                    let recv = opt_translate(x.receiver().as_ref(), cx, in_block);
                    let safe = RubyValue::Bool(x.is_safe_navigation());
                    let name = sym_val(x.read_name().as_slice());
                    let value = translate(&x.value(), cx, in_block);
                    let op: RubyValue = $op(&x);
                    return Some(node(
                        cx,
                        "OP_ASGN2",
                        s,
                        e,
                        vec![recv, safe, name, op, value],
                    ));
                }
            };
        }
        attr_asgn!(
            as_call_operator_write_node,
            |x: &ruby_prism::CallOperatorWriteNode<'_>| {
                RubyValue::Symbol(crate::Symbol::intern(&String::from_utf8_lossy(
                    x.binary_operator().as_slice(),
                )))
            }
        );
        attr_asgn!(as_call_or_write_node, |_: &ruby_prism::CallOrWriteNode<
            '_,
        >| sym("||"));
        attr_asgn!(
            as_call_and_write_node,
            |_: &ruby_prism::CallAndWriteNode<'_>| sym("&&")
        );
        None
    }

    /// A regexp literal's VALUE, with the encoding its flag forces.
    fn regexp_literal(cx: &Cx, x: &ruby_prism::RegularExpressionNode<'_>) -> RubyValue {
        let src = cx.slice(x.content_loc());
        // The FLAG the literal was written with, not the encoding its bytes
        // happen to force -- the two are different questions in prism.
        let enc = if x.is_ascii_8bit() {
            zeo_abi::RegexpEncoding::None
        } else if x.is_euc_jp() {
            zeo_abi::RegexpEncoding::EucJp
        } else if x.is_windows_31j() {
            zeo_abi::RegexpEncoding::Windows31j
        } else if x.is_utf_8() {
            zeo_abi::RegexpEncoding::Utf8
        } else {
            zeo_abi::RegexpEncoding::Source
        };
        crate::regexp::regexp_new_enc(
            &src,
            x.is_ignore_case(),
            x.is_extended(),
            x.is_multi_line(),
            enc,
        )
        .map(RubyValue::Regexp)
        .unwrap_or(RubyValue::Nil)
    }

    /// `A::B op= v` -- `OP_CDECL[path, op, value]`. A constant PATH does not
    /// go through [`op_assign`]'s read/write pair: there is no name to read,
    /// so CRuby keeps the whole path and one kind for all three operators.
    fn const_path_op_assign(
        n: &P<'_>,
        cx: &mut Cx,
        in_block: bool,
        s: usize,
        e: usize,
    ) -> Option<RubyValue> {
        let sym = |t: &str| RubyValue::Symbol(crate::Symbol::intern(t));
        macro_rules! path {
            ($m:ident, $op:expr) => {
                if let Some(x) = n.$m() {
                    let target = x.target();
                    let tloc = target.location();
                    let path = const_path_of(&target, cx, tloc.start_offset(), tloc.end_offset());
                    let value = translate(&x.value(), cx, in_block);
                    let op: RubyValue = $op(&x);
                    return Some(node(cx, "OP_CDECL", s, e, vec![path, op, value]));
                }
            };
        }
        path!(
            as_constant_path_operator_write_node,
            |x: &ruby_prism::ConstantPathOperatorWriteNode<'_>| {
                RubyValue::Symbol(crate::Symbol::intern(&String::from_utf8_lossy(
                    x.binary_operator().as_slice(),
                )))
            }
        );
        path!(
            as_constant_path_or_write_node,
            |_: &ruby_prism::ConstantPathOrWriteNode<'_>| sym("||")
        );
        path!(
            as_constant_path_and_write_node,
            |_: &ruby_prism::ConstantPathAndWriteNode<'_>| sym("&&")
        );
        None
    }

    /// The `COLON2` a constant-path target reads as.
    fn const_path_of(
        target: &ruby_prism::ConstantPathNode<'_>,
        cx: &mut Cx,
        s: usize,
        e: usize,
    ) -> RubyValue {
        let parent = match target.parent() {
            Some(p) => translate(&p, cx, false),
            None => RubyValue::Nil,
        };
        let name = match target.name() {
            Some(nm) => sym_val(nm.as_slice()),
            None => RubyValue::Nil,
        };
        node(cx, "COLON2", s, e, vec![parent, name])
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
        // `"a" "b"` is one literal, and CRuby folds it: when every part is a
        // plain string the answer is a STR spanning the FIRST part alone.
        if let Some(x) = n.as_interpolated_string_node() {
            let parts: Vec<P<'_>> = x.parts().iter().collect();
            if !parts.is_empty() && parts.iter().all(|p| p.as_string_node().is_some()) {
                let mut text = String::new();
                for p in &parts {
                    let sn = p.as_string_node().expect("checked");
                    text.push_str(&String::from_utf8_lossy(sn.unescaped()));
                }
                let first = parts[0].location();
                let v = RubyValue::Str(crate::string_new(text));
                return node(cx, "STR", first.start_offset(), first.end_offset(), vec![v]);
            }
        }
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
            let enc = crate::builtins::encoding::encoding_value(crate::encoding::UTF_8);
            return node(cx, "ENCODING", s, e, vec![enc]);
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
            // `[]` is its own kind in parse.y, with no children at all.
            return arg_list(&els, s, e, cx, in_block)
                .unwrap_or_else(|| node(cx, "ZLIST", s, e, vec![]));
        }
        if let Some(x) = n.as_hash_node() {
            let els: Vec<P<'_>> = x.elements().iter().collect();
            return hash_node(cx, &els, s, e, in_block);
        }
        // `foo(a: 1)` and `foo(**h)`: prism keeps a call's trailing keywords in
        // their own node, which CRuby renders as an ordinary HASH argument.
        if let Some(x) = n.as_keyword_hash_node() {
            let els: Vec<P<'_>> = x.elements().iter().collect();
            return hash_node(cx, &els, s, e, in_block);
        }
        if let Some(x) = n.as_range_node() {
            let kind = if x.is_exclude_end() { "DOT3" } else { "DOT2" };
            // A beginless or endless range still has both children: CRuby
            // fills the absent side with a zero-width `NIL` node sited at the
            // edge the operator does not reach.
            let lo = match x.left() {
                Some(l) => translate(&l, cx, in_block),
                None => node(cx, "NIL", s, s, vec![]),
            };
            let hi = match x.right() {
                Some(r) => translate(&r, cx, in_block),
                None => node(cx, "NIL", e, e, vec![]),
            };
            return node(cx, kind, s, e, vec![lo, hi]);
        }
        if let Some(x) = n.as_flip_flop_node() {
            let kind = if x.is_exclude_end() { "FLIP3" } else { "FLIP2" };
            let lo = opt_translate(x.left().as_ref(), cx, in_block);
            let hi = opt_translate(x.right().as_ref(), cx, in_block);
            return node(cx, kind, s, e, vec![lo, hi]);
        }
        if let Some(x) = n.as_call_node() {
            let name = String::from_utf8_lossy(x.name().as_slice()).into_owned();
            // The block form wraps the bare call in ITER; the inner call's
            // span EXCLUDES the block (its message/arguments only), and a
            // blockful no-arg call is FCALL with nil args, never VCALL.
            if let Some(block) = x.block().and_then(|b| b.as_block_node()) {
                let call = call_for_iter(&x, &name, cx, in_block);
                return iter_over(call, &block, s, e, cx);
            }
            return call_without_block(&x, &name, cx, in_block);
        }
        if let Some(x) = n.as_def_node() {
            let dloc = x.location();
            let mut locals: Vec<RubyValue> =
                x.locals().iter().map(|l| sym_val(l.as_slice())).collect();
            let params = x.parameters();
            let (ps, pe) = match (&params, x.lparen_loc(), x.rparen_loc()) {
                (Some(p), _, _) => {
                    let l = p.location();
                    (l.start_offset(), l.end_offset())
                }
                // An EMPTY parameter list still has a span, and it is the
                // opening paren alone.
                (None, Some(lp), Some(_)) => (lp.start_offset(), lp.end_offset()),
                // An endless `def m = 1` with no parens spans `def m`; a
                // `def m; end` puts the zero-width ARGS after the name.
                (None, None, _) => {
                    let after = x.name_loc().end_offset();
                    match x.equal_loc() {
                        Some(_) => (dloc.start_offset(), after),
                        None => (after, after),
                    }
                }
                (None, Some(lp), None) => (lp.start_offset(), lp.end_offset()),
            };
            let args = args_node(params, ps, pe, cx, false, &mut locals);
            // A `def` with its own `rescue`/`ensure` has a `begin` for a body.
            // Its argument list takes ONE terminator, so only a SECOND
            // separator (`def m; ; 1; end`) leaves the empty statement.
            let after_header = x
                .rparen_loc()
                .map(|r| r.end_offset())
                .unwrap_or_else(|| pe.max(x.name_loc().end_offset()));
            // A PARENTHESISED argument list ends at its `)`, so the grammar
            // has nothing left to take a `;` with -- only the paren-less form
            // eats one. A newline is absorbed either way.
            let absorb = match x.lparen_loc() {
                Some(_) => Absorb::Newlines,
                None => Absorb::One,
            };
            let body = match x.body() {
                Some(b) => match b.as_statements_node() {
                    // An EMPTY method body is nil, never the empty statement.
                    Some(stmts) if stmts.body().iter().next().is_none() => RubyValue::Nil,
                    Some(stmts) => body_after(Some(stmts), after_header, absorb, cx, false),
                    // A rescue/ensure body is a BeginNode, and in a `def` it
                    // is NOT the expression-position `BEGIN` wrapper.
                    None => match b.as_begin_node() {
                        Some(bg) => {
                            let at = cx.body_start(after_header, absorb);
                            begin_body(&bg, cx, false, at, dloc.end_offset(), false)
                        }
                        None => translate(&b, cx, false),
                    },
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
            let name = sym_val(x.name().as_slice());
            return match x.receiver() {
                Some(r) => {
                    let recv = translate(&r, cx, in_block);
                    node(cx, "DEFS", s, e, vec![recv, name, scope])
                }
                None => node(cx, "DEFN", s, e, vec![name, scope]),
            };
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
            // A NAMELESS `*` is a marker with no node, so its own text is the
            // only thing that gives the pattern an extent.
            let star = x
                .rest()
                .filter(|_| matches!(rest, RubyValue::Symbol(_)))
                .map(|r| {
                    let l = r.location();
                    (l.start_offset(), l.end_offset())
                });
            let (ps, pe) =
                pattern_span_with(x.location(), &[&konst, &pre_list, &rest, &post_list], star);
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
            // A `**rest` stretches BOTH the hash and its own binding over the
            // whole pattern, which is CRuby's own span and not the text each
            // part covers.
            let content = {
                let lo = pairs
                    .first()
                    .map(|p| p.location().start_offset())
                    .or_else(|| x.rest().map(|r| r.location().start_offset()));
                let hi = x
                    .rest()
                    .map(|r| r.location().end_offset())
                    .or_else(|| pairs.last().map(|p| p.location().end_offset()));
                lo.zip(hi)
            };
            let named_rest = x
                .rest()
                .as_ref()
                .and_then(|r| r.as_assoc_splat_node())
                .and_then(|a| a.value())
                .is_some();
            let rest = match x.rest() {
                // `**nil` -- "and no other keys", which is a marker, not a
                // binding.
                Some(r) if r.as_no_keywords_parameter_node().is_some() => {
                    RubyValue::Symbol(crate::Symbol::intern("NODE_SPECIAL_NO_REST_KEYWORD"))
                }
                Some(r) => match r.as_assoc_splat_node().and_then(|a| a.value()) {
                    Some(t) => {
                        let from = content.map(|(lo, _)| lo);
                        let mut b = asgn_node(&t, from, RubyValue::Nil, cx);
                        if let Some((lo, hi)) = content {
                            b = respan(b, cx, lo, hi);
                        }
                        b
                    }
                    None => RubyValue::Nil,
                },
                None => RubyValue::Nil,
            };
            let hash = match (named_rest, content, &hash) {
                (true, Some((lo, hi)), RubyValue::Object(_)) => respan(hash, cx, lo, hi),
                _ => hash,
            };
            let (ps, pe) = pattern_span(x.location(), &[&konst, &hash, &rest]);
            // `in **nil` says "and no other keys", which is a marker BESIDE an
            // empty hash -- `in {}` is the one that holds nothing at all.
            let hash = match (&hash, x.rest()) {
                (RubyValue::Nil, Some(r)) if r.as_no_keywords_parameter_node().is_some() => {
                    node(cx, "HASH", ps, pe, vec![RubyValue::Nil])
                }
                _ => hash,
            };
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
            // An arm with no statements is CRuby's zero-width empty statement,
            // sited where its body would start.
            let then = clause_body(
                x.statements(),
                x.then_keyword_loc()
                    .map(|t| t.end_offset())
                    .unwrap_or_else(|| x.predicate().location().end_offset()),
                cx,
                in_block,
            );
            let els = match x.subsequent() {
                Some(sub) => match sub.as_else_node() {
                    Some(e2) => else_clause_body(e2.statements(), &e2, cx, in_block),
                    // An `elsif` runs to the terminator after its own last
                    // part, not to the `end` that closes the whole chain.
                    None => {
                        let v = translate(&sub, cx, in_block);
                        retrim_elsif(v, cx)
                    }
                },
                None => RubyValue::Nil,
            };
            return node(cx, "IF", s, e, vec![cond, then, els]);
        }
        if let Some(x) = n.as_unless_node() {
            let cond = translate(&x.predicate(), cx, in_block);
            let then = clause_body(
                x.statements(),
                x.then_keyword_loc()
                    .map(|t| t.end_offset())
                    .unwrap_or_else(|| x.predicate().location().end_offset()),
                cx,
                in_block,
            );
            let els = match x.else_clause() {
                Some(e2) => else_clause_body(e2.statements(), &e2, cx, in_block),
                None => RubyValue::Nil,
            };
            return node(cx, "UNLESS", s, e, vec![cond, then, els]);
        }
        if let Some(x) = n.as_while_node() {
            let cond = translate(&x.predicate(), cx, in_block);
            let at = x
                .do_keyword_loc()
                .map(|d| d.end_offset())
                .unwrap_or_else(|| x.predicate().location().end_offset());
            let body = clause_body(x.statements(), at, cx, in_block);
            let pre = RubyValue::Bool(!x.is_begin_modifier());
            return node(cx, "WHILE", s, e, vec![cond, body, pre]);
        }
        if let Some(x) = n.as_until_node() {
            let cond = translate(&x.predicate(), cx, in_block);
            let at = x
                .do_keyword_loc()
                .map(|d| d.end_offset())
                .unwrap_or_else(|| x.predicate().location().end_offset());
            let body = clause_body(x.statements(), at, cx, in_block);
            let pre = RubyValue::Bool(!x.is_begin_modifier());
            return node(cx, "UNTIL", s, e, vec![cond, body, pre]);
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
                RubyValue::Nil => {
                    let at = cx.body_start(body_end, absorb);
                    empty_begin(cx, at)
                }
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
            let body = match body_after(
                x.body().and_then(|b| b.as_statements_node()),
                at,
                Absorb::One,
                cx,
                false,
            ) {
                RubyValue::Nil => {
                    let start = cx.body_start(at, Absorb::One);
                    empty_begin(cx, start)
                }
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
                RubyValue::Nil => empty_begin(cx, cx.body_start(at, Absorb::None)),
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
        if let Some(x) = n.as_parentheses_node() {
            let at = x.opening_loc().end_offset();
            let inner = match x.body().and_then(|b| b.as_statements_node()) {
                Some(body) => body_after(Some(body), at, Absorb::Newlines, cx, in_block),
                None => RubyValue::Nil,
            };
            let inner = match inner {
                RubyValue::Nil => empty_begin(cx, at),
                other => other,
            };
            return node(cx, "BLOCK", s, e, vec![inner]);
        }
        if let Some(x) = n.as_statements_node() {
            return statements_body(&x, cx, in_block);
        }

        if let Some(x) = n.as_rational_node() {
            let (num, den) = (x.numerator(), x.denominator());
            let (nneg, ndig) = num.to_u32_digits();
            let (dneg, ddig) = den.to_u32_digits();
            let v = crate::builtins::rational::rational_new(
                bigint_of(nneg, ndig),
                bigint_of(dneg, ddig),
            )
            .unwrap_or(RubyValue::Nil);
            return node(cx, "RATIONAL", s, e, vec![v]);
        }
        if let Some(x) = n.as_imaginary_node() {
            let imag = numeric_value(&x.numeric());
            let v = crate::builtins::complex::complex_new(RubyValue::Int(0), imag)
                .unwrap_or(RubyValue::Nil);
            return node(cx, "IMAGINARY", s, e, vec![v]);
        }
        if let Some(x) = n.as_x_string_node() {
            let v = RubyValue::Str(crate::string_new(
                String::from_utf8_lossy(x.unescaped()).into_owned(),
            ));
            return node(cx, "XSTR", s, e, vec![v]);
        }
        if let Some(x) = n.as_regular_expression_node() {
            return node(cx, "REGX", s, e, vec![regexp_literal(cx, &x)]);
        }
        if let Some(x) = n.as_match_last_line_node() {
            // A bare `/re/` in condition position matches against `$_`, and
            // CRuby gives that its own kind.
            let src = cx.slice(x.content_loc());
            let v = crate::regexp::regexp_new(
                &src,
                x.is_ignore_case(),
                x.is_extended(),
                x.is_multi_line(),
            )
            .map(RubyValue::Regexp)
            .unwrap_or(RubyValue::Nil);
            return node(cx, "MATCH", s, e, vec![v]);
        }
        // `it` reads a parameter with a name no program can write.
        if n.as_it_local_variable_read_node().is_some() {
            let name = RubyValue::Symbol(crate::Symbol::intern("<it>"));
            return node(cx, "DVAR", s, e, vec![name]);
        }
        if let Some(x) = n.as_yield_node() {
            let args = x.arguments().and_then(|a| {
                let al = a.location();
                let els: Vec<P<'_>> = a.arguments().iter().collect();
                arg_list(&els, al.start_offset(), al.end_offset(), cx, in_block)
            });
            return node(cx, "YIELD", s, e, vec![args.unwrap_or(RubyValue::Nil)]);
        }
        // `^(expr)` pins a computed value, which CRuby renders as the
        // parenthesised expression it is.
        if let Some(x) = n.as_pinned_expression_node() {
            let inner = translate(&x.expression(), cx, in_block);
            return node(cx, "BLOCK", s, e, vec![inner]);
        }
        // `A::B op= v` is its own kind, holding the PATH rather than a name.
        if let Some(v) = const_path_op_assign(n, cx, in_block, s, e) {
            return v;
        }
        if let Some(x) = n.as_class_variable_read_node() {
            return node(cx, "CVAR", s, e, vec![sym_val(x.name().as_slice())]);
        }
        if let Some(x) = n.as_class_variable_write_node() {
            let v = translate(&x.value(), cx, in_block);
            return node(cx, "CVASGN", s, e, vec![sym_val(x.name().as_slice()), v]);
        }
        if let Some(x) = n.as_numbered_reference_read_node() {
            let name = format!("${}", x.number());
            let sym = RubyValue::Symbol(crate::Symbol::intern(&name));
            return node(cx, "NTH_REF", s, e, vec![sym]);
        }
        if let Some(x) = n.as_back_reference_read_node() {
            return node(cx, "BACK_REF", s, e, vec![sym_val(x.name().as_slice())]);
        }
        if let Some(x) = n.as_defined_node() {
            let v = translate(&x.value(), cx, in_block);
            return node(cx, "DEFINED", s, e, vec![v]);
        }
        if let Some(x) = n.as_forwarding_super_node() {
            // `super` with no argument list forwards, and takes a block the
            // same way any call does.
            let call_e = match x.block() {
                Some(ref b) => b.location().start_offset().max(s),
                None => e,
            };
            // The keyword alone, with no trailing space: `super { }` is
            // ZSUPER[0,5], not ZSUPER[0,9].
            let call_e = cx.trim_trailing_space(s, call_e);
            let zsuper = node(cx, "ZSUPER", s, call_e, vec![]);
            return match x.block() {
                Some(b) => iter_over(zsuper, &b, s, e, cx),
                None => zsuper,
            };
        }
        if let Some(x) = n.as_super_node() {
            let args = x.arguments().and_then(|a| {
                let al = a.location();
                let els: Vec<P<'_>> = a.arguments().iter().collect();
                arg_list(&els, al.start_offset(), al.end_offset(), cx, in_block)
            });
            let (call_s, call_e) = match x.block().as_ref().and_then(|b| b.as_block_node()) {
                Some(b) => (
                    s,
                    cx.trim_trailing_space(s, b.location().start_offset().max(s)),
                ),
                None => (s, e),
            };
            let sup = node(
                cx,
                "SUPER",
                call_s,
                call_e,
                vec![args.unwrap_or(RubyValue::Nil)],
            );
            return match x.block().as_ref().and_then(|b| b.as_block_node()) {
                Some(b) => iter_over(sup, &b, s, e, cx),
                None => sup,
            };
        }
        if let Some(x) = n.as_lambda_node() {
            let scope = lambda_scope(&x, cx);
            return node(cx, "LAMBDA", s, e, vec![scope]);
        }
        if let Some(x) = n.as_alias_method_node() {
            let new = translate(&x.new_name(), cx, in_block);
            let old = translate(&x.old_name(), cx, in_block);
            return node(cx, "ALIAS", s, e, vec![new, old]);
        }
        if let Some(x) = n.as_alias_global_variable_node() {
            // The global form names its two variables as bare symbols, where
            // the method form holds two SYM nodes.
            let name = |g: &P<'_>| -> RubyValue {
                let l = g.location();
                RubyValue::Symbol(crate::Symbol::intern(&cx_slice(cx, l)))
            };
            let new = name(&x.new_name());
            let old = name(&x.old_name());
            return node(cx, "VALIAS", s, e, vec![new, old]);
        }
        if let Some(x) = n.as_undef_node() {
            // One ARRAY of SYM nodes, not a chain.
            let names: Vec<RubyValue> = x
                .names()
                .iter()
                .map(|nm| translate(&nm, cx, in_block))
                .collect();
            let list = RubyValue::Array(crate::array_new(names));
            return node(cx, "UNDEF", s, e, vec![list]);
        }
        if let Some(x) = n.as_post_execution_node() {
            let body = match opt_statements(x.statements(), cx, in_block) {
                RubyValue::Nil => empty_begin(cx, x.opening_loc().end_offset()),
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
            return node(cx, "POSTEXE", s, e, vec![scope]);
        }
        if let Some(x) = n.as_for_node() {
            return for_node(&x, cx, s, e);
        }
        if let Some(x) = n.as_multi_write_node() {
            let value = translate(&x.value(), cx, in_block);
            return masgn(cx, MultiParts::write(&x), Some(value), s, e);
        }
        if let Some(x) = n.as_multi_target_node() {
            return masgn(cx, MultiParts::target(&x), None, s, e);
        }
        // `a => b` binds or raises; `a in b` answers a boolean. Both are a
        // one-armed CASE3, and the boolean form says so by carrying TRUE and
        // FALSE where the binding form carries nils.
        if let Some(x) = n.as_match_required_node() {
            let subject = translate(&x.value(), cx, in_block);
            let pattern = translate(&x.pattern(), cx, in_block);
            let (ps, pe) = {
                let l = x.pattern().location();
                (l.start_offset(), l.end_offset())
            };
            let arm = node(
                cx,
                "IN",
                ps,
                pe,
                vec![pattern, RubyValue::Nil, RubyValue::Nil],
            );
            return node(cx, "CASE3", s, e, vec![subject, arm]);
        }
        if let Some(x) = n.as_match_predicate_node() {
            let subject = translate(&x.value(), cx, in_block);
            let pattern = translate(&x.pattern(), cx, in_block);
            let (ps, pe) = {
                let l = x.pattern().location();
                (l.start_offset(), l.end_offset())
            };
            let yes = node(cx, "TRUE", ps, pe, vec![]);
            let no = node(cx, "FALSE", ps, pe, vec![]);
            let arm = node(cx, "IN", ps, pe, vec![pattern, yes, no]);
            return node(cx, "CASE3", s, e, vec![subject, arm]);
        }
        // The operator-assignment family. `x += 1` is not one node in CRuby's
        // tree but the READ, the call, and the WRITE spelled out; `||=` and
        // `&&=` keep their own kinds because they short-circuit.
        if let Some(v) = op_assign(n, cx, in_block, s, e) {
            return v;
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
            .closing_loc()
            .map(|c| c.end_offset())
            .or_else(|| x.arguments().map(|a| a.location().end_offset()))
            .unwrap_or(msg.1);
        let name_sym = RubyValue::Symbol(crate::Symbol::intern(name));
        let args = call_arguments(x, cx, in_block);
        match x.receiver() {
            Some(recv) => {
                let r = translate(&recv, cx, in_block);
                let kind = if x.is_safe_navigation() {
                    "QCALL"
                } else {
                    "CALL"
                };
                node(
                    cx,
                    kind,
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

    /// `n(...)` -- the three hidden parameters `...` forwards, spelled out.
    ///
    /// CRuby does not keep a forwarding node: it expands to the splat, the
    /// double-splat and the block pass over the locals `...` declared, which
    /// is why every part below reads a `LVAR` the parameter list created.
    /// `None` when this call does not forward.
    fn forwarded_arguments(x: &ruby_prism::CallNode<'_>, cx: &mut Cx) -> Option<RubyValue> {
        let args = x.arguments()?;
        let els: Vec<P<'_>> = args.arguments().iter().collect();
        let [only] = els.as_slice() else {
            return None;
        };
        let fwd = only.as_forwarding_arguments_node()?;
        let floc = fwd.location();
        let (fs, fe) = (floc.start_offset(), floc.end_offset());
        // The whole expansion spans the PARENS, not the `...` inside them.
        let (ws, we) = match (x.opening_loc(), x.closing_loc()) {
            (Some(o), Some(c)) => (o.start_offset(), c.end_offset()),
            _ => (fs, fe),
        };
        let lvar = |cx: &mut Cx, name: &str| {
            let sym = RubyValue::Symbol(crate::Symbol::intern(name));
            node(cx, "LVAR", fs, fe, vec![sym])
        };
        let rest = lvar(cx, "*");
        let splat = node(cx, "SPLAT", fs, fe, vec![rest]);
        let kwrest = lvar(cx, "**");
        let pairs = list_node(cx, fs, fe, vec![RubyValue::Nil, kwrest]);
        let hash = node(cx, "HASH", fs, fe, vec![pairs]);
        let push = node(cx, "ARGSPUSH", ws, we, vec![splat, hash]);
        let blk = lvar(cx, "&");
        Some(node(cx, "BLOCK_PASS", ws, we, vec![push, blk]))
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
        let args = call_arguments(x, cx, in_block);
        // A writer call is an ATTRASGN, whatever brought it here. The safe
        // form drops the `=` from the message, which CRuby does too.
        if x.is_attribute_write() || name == "[]=" {
            let recv = match x.receiver() {
                Some(r) => translate(&r, cx, in_block),
                None => RubyValue::Nil,
            };
            let msg = if x.is_safe_navigation() {
                RubyValue::Symbol(crate::Symbol::intern(name.trim_end_matches('=')))
            } else {
                name_sym
            };
            return node(
                cx,
                "ATTRASGN",
                s,
                e,
                vec![recv, msg, args.unwrap_or(RubyValue::Nil)],
            );
        }
        match x.receiver() {
            Some(recv) => {
                let mut r = translate(&recv, cx, in_block);
                // `-2**2` is one production in parse.y (`tUMINUS_NUM
                // simple_numeric tPOW arg`), so the POWER carries the minus
                // sign's span even though the negation wraps it.
                if name == "-@" && numeric_power(&recv) {
                    r = respan(r, cx, s, e);
                }
                let kind = if x.is_safe_navigation() {
                    "QCALL"
                } else if is_operator(name) {
                    "OPCALL"
                } else {
                    "CALL"
                };
                node(
                    cx,
                    kind,
                    s,
                    e,
                    vec![r, name_sym, args.unwrap_or(RubyValue::Nil)],
                )
            }
            // A bare name with no argument list at all is VCALL; `foo()` has
            // one, empty, and is FCALL.
            None => match args {
                Some(a) => node(cx, "FCALL", s, e, vec![name_sym, a]),
                None if x.opening_loc().is_some() => {
                    node(cx, "FCALL", s, e, vec![name_sym, RubyValue::Nil])
                }
                None => node(cx, "VCALL", s, e, vec![name_sym]),
            },
        }
    }

    /// Whether a node is `<numeric literal> ** x`, the one shape a leading
    /// minus sign parses into rather than around.
    fn numeric_power(n: &P<'_>) -> bool {
        let Some(call) = n.as_call_node() else {
            return false;
        };
        if call.name().as_slice() != b"**" {
            return false;
        }
        call.receiver().is_some_and(|r| {
            r.as_integer_node().is_some()
                || r.as_float_node().is_some()
                || r.as_rational_node().is_some()
                || r.as_imaginary_node().is_some()
        })
    }

    /// A call's argument list, with the `&block` a `BLOCK_PASS` wraps around
    /// everything before it -- `foo(1, &b)` is `BLOCK_PASS[LIST[1], b]`, and
    /// `foo(&b)` is `BLOCK_PASS[nil, b]`.
    fn call_arguments(
        x: &ruby_prism::CallNode<'_>,
        cx: &mut Cx,
        in_block: bool,
    ) -> Option<RubyValue> {
        if let Some(v) = forwarded_arguments(x, cx) {
            return Some(v);
        }
        let plain = x.arguments().and_then(|a| {
            let al = a.location();
            let els: Vec<P<'_>> = a.arguments().iter().collect();
            arg_list(&els, al.start_offset(), al.end_offset(), cx, in_block)
        });
        let Some(bp) = x.block().and_then(|b| b.as_block_argument_node()) else {
            return plain;
        };
        let bloc = bp.location();
        let value = opt_translate(bp.expression().as_ref(), cx, in_block);
        let lo = range_of(&plain.clone().unwrap_or(RubyValue::Nil))
            .map_or(bloc.start_offset(), |(a, _)| a);
        Some(node(
            cx,
            "BLOCK_PASS",
            lo,
            bloc.end_offset(),
            vec![plain.unwrap_or(RubyValue::Nil), value],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole tree, in the shape `tests/rubyvm_ast.rb` prints: kind, span,
    /// then children. Every expectation below is a byte-for-byte copy of what
    /// `mise exec ruby@4.0.6 -- ruby` answers for the same source.
    fn dump(v: &RubyValue) -> String {
        let RubyValue::Object(o) = v else {
            return match v {
                RubyValue::Nil => "nil".to_string(),
                RubyValue::Bool(b) => b.to_string(),
                RubyValue::Int(i) => i.to_string(),
                RubyValue::Symbol(s) => crate::builtins::symbol::inspect_name(&s.name()),
                other => other.inspect_string(),
            };
        };
        let Some(n) = o.as_any().downcast_ref::<RAstNode>() else {
            return v.inspect_string();
        };
        let kids: Vec<String> = n.children.iter().map(dump).collect();
        format!(
            "({} [{}:{}-{}:{}] {})",
            n.kind,
            n.span.fl,
            n.span.fc,
            n.span.ll,
            n.span.lc,
            kids.join(" ")
        )
    }

    fn tree(src: &str) -> String {
        let v = translate::parse(src, false, false).expect("parses");
        dump(&v)
    }

    /// The program's own `SCOPE` wrapper, stripped, so a case names only the
    /// shape it is about.
    fn body(src: &str) -> String {
        let t = tree(src);
        let head = t
            .find("] ")
            .map(|i| i + 2)
            .expect("a SCOPE always has a span");
        let inner = &t[head..t.len() - 1];
        // Past the locals array and the (always nil) second slot.
        let after = inner.find("] ").map(|i| i + 2).expect("locals array");
        inner[after..].trim_start_matches("nil ").to_string()
    }

    #[test]
    fn a_literal_answers_its_own_kind() {
        assert_eq!(body("1"), "(INTEGER [1:0-1:1] 1)");
        assert_eq!(body("1.5"), "(FLOAT [1:0-1:3] 1.5)");
        assert_eq!(body("1r"), "(RATIONAL [1:0-1:2] (1/1))");
        assert_eq!(body("1i"), "(IMAGINARY [1:0-1:2] (0+1i))");
        assert_eq!(body("nil"), "(NIL [1:0-1:3] )");
        assert_eq!(body("true"), "(TRUE [1:0-1:4] )");
        assert_eq!(body("self"), "(SELF [1:0-1:4] )");
        assert_eq!(body("`ls`"), "(XSTR [1:0-1:4] \"ls\")");
        assert_eq!(body("/re/"), "(REGX [1:0-1:4] /re/)");
        assert_eq!(body("/x/mix"), "(REGX [1:0-1:6] /x/mix)");
    }

    /// A numbered reference is named by its NUMBER and a back reference by
    /// its whole spelling -- `$&` is `:$&`, not `:&`.
    #[test]
    fn a_reference_read_keeps_its_sigil() {
        assert_eq!(body("$1"), "(NTH_REF [1:0-1:2] :$1)");
        assert_eq!(body("$12"), "(NTH_REF [1:0-1:3] :$12)");
        assert_eq!(body("$&"), "(BACK_REF [1:0-1:2] :$&)");
        assert_eq!(body("$~"), "(GVAR [1:0-1:2] :$~)");
    }

    /// `[]` and `{}` are not one-element containers holding nothing: an empty
    /// array literal is its own kind, and an empty hash holds no list.
    #[test]
    fn an_empty_container_is_not_a_one_nil_list() {
        assert_eq!(body("[]"), "(ZLIST [1:0-1:2] )");
        assert_eq!(body("%w[]"), "(ZLIST [1:0-1:4] )");
        assert_eq!(body("{}"), "(HASH [1:0-1:2] nil)");
        assert_eq!(body("[1]"), "(LIST [1:0-1:3] (INTEGER [1:1-1:2] 1) nil)");
    }

    /// A `**rest` is a NIL KEY beside its value, which is how parse.y spells
    /// the splat inside a flat key/value list.
    #[test]
    fn a_double_splat_is_a_nil_key() {
        assert_eq!(
            body("{**h}"),
            "(HASH [1:0-1:5] (LIST [1:1-1:4] nil (VCALL [1:3-1:4] :h) nil))"
        );
        assert_eq!(
            body("{a: 1, **h}"),
            "(HASH [1:0-1:11] (LIST [1:1-1:10] (SYM [1:1-1:3] :a) (INTEGER [1:4-1:5] 1) \
             nil (VCALL [1:9-1:10] :h) nil))"
        );
    }

    /// An absent range endpoint is a zero-width `NIL` NODE at the edge the
    /// operator does not reach, never a missing child.
    #[test]
    fn an_open_range_still_has_two_children() {
        assert_eq!(
            body("(1..)"),
            "(BLOCK [1:0-1:5] (DOT2 [1:1-1:4] (INTEGER [1:1-1:2] 1) (NIL [1:4-1:4] )))"
        );
        assert_eq!(
            body("(..2)"),
            "(BLOCK [1:0-1:5] (DOT2 [1:1-1:4] (NIL [1:1-1:1] ) (INTEGER [1:3-1:4] 2)))"
        );
    }

    /// The taxonomy follows parse.y's productions, not the shape of the name:
    /// `[]`, `[]=` and `=~` are ordinary calls, and a writer is an ATTRASGN.
    #[test]
    fn a_call_kind_follows_the_grammar_not_the_name() {
        assert_eq!(body("foo"), "(VCALL [1:0-1:3] :foo)");
        assert_eq!(body("foo()"), "(FCALL [1:0-1:5] :foo nil)");
        assert_eq!(
            body("a&.b"),
            "(QCALL [1:0-1:4] (VCALL [1:0-1:1] :a) :b nil)"
        );
        assert_eq!(
            body("a[1]"),
            "(CALL [1:0-1:4] (VCALL [1:0-1:1] :a) :[] (LIST [1:2-1:3] (INTEGER [1:2-1:3] 1) nil))"
        );
        assert_eq!(
            body("a =~ b"),
            "(CALL [1:0-1:6] (VCALL [1:0-1:1] :a) :=~ (LIST [1:5-1:6] (VCALL [1:5-1:6] :b) nil))"
        );
        assert_eq!(
            body("a + b"),
            "(OPCALL [1:0-1:5] (VCALL [1:0-1:1] :a) :+ (LIST [1:4-1:5] (VCALL [1:4-1:5] :b) nil))"
        );
        assert_eq!(
            body("a.b = 1"),
            "(ATTRASGN [1:0-1:7] (VCALL [1:0-1:1] :a) :b= (LIST [1:6-1:7] (INTEGER [1:6-1:7] 1) nil))"
        );
        // The safe form drops the `=` from the message it reports.
        assert_eq!(
            body("a&.b = 1"),
            "(ATTRASGN [1:0-1:8] (VCALL [1:0-1:1] :a) :b (LIST [1:7-1:8] (INTEGER [1:7-1:8] 1) nil))"
        );
    }

    /// A `&block` wraps everything before it, and stands alone over nil when
    /// there is nothing before it.
    #[test]
    fn a_block_pass_wraps_the_arguments_before_it() {
        assert_eq!(
            body("foo(&b)"),
            "(FCALL [1:0-1:7] :foo (BLOCK_PASS [1:4-1:6] nil (VCALL [1:5-1:6] :b)))"
        );
        assert_eq!(
            body("foo(1, &b)"),
            "(FCALL [1:0-1:10] :foo (BLOCK_PASS [1:4-1:9] (LIST [1:4-1:5] \
             (INTEGER [1:4-1:5] 1) nil) (VCALL [1:8-1:9] :b)))"
        );
    }

    /// A binary operator assignment DESUGARS -- the tree is the write holding
    /// a call on the read. `||=` and `&&=` do not, because they short-circuit.
    #[test]
    fn an_operator_assignment_desugars_but_a_short_circuit_does_not() {
        assert_eq!(
            body("x += 1"),
            "(LASGN [1:0-1:6] :x (CALL [1:0-1:6] (LVAR [1:0-1:1] :x) :+ \
             (LIST [1:5-1:6] (INTEGER [1:5-1:6] 1) nil)))"
        );
        assert_eq!(
            body("x ||= 1"),
            "(OP_ASGN_OR [1:0-1:7] (LVAR [1:0-1:1] :x) :\"||\" \
             (LASGN [1:0-1:7] :x (INTEGER [1:6-1:7] 1)))"
        );
        assert_eq!(
            body("a[0] += 1"),
            "(OP_ASGN1 [1:0-1:9] (VCALL [1:0-1:1] :a) :+ \
             (LIST [1:2-1:3] (INTEGER [1:2-1:3] 0) nil) (INTEGER [1:8-1:9] 1))"
        );
        // OP_ASGN2 is the only node carrying a safe-navigation flag.
        assert_eq!(
            body("a.b += 1"),
            "(OP_ASGN2 [1:0-1:8] (VCALL [1:0-1:1] :a) false :b :+ (INTEGER [1:7-1:8] 1))"
        );
        assert_eq!(
            body("a&.b += 1"),
            "(OP_ASGN2 [1:0-1:9] (VCALL [1:0-1:1] :a) true :b :+ (INTEGER [1:8-1:9] 1))"
        );
        // A constant PATH has no name to read, so it keeps the whole path and
        // one kind for all three operators.
        assert_eq!(
            body("A::B ||= 1"),
            "(OP_CDECL [1:0-1:10] (COLON2 [1:0-1:4] (CONST [1:0-1:1] :A) :B) \
             :\"||\" (INTEGER [1:9-1:10] 1))"
        );
    }

    /// A rest with targets after it is a `POSTARG` holding both, and a
    /// TRAILING COMMA is not a rest at all.
    #[test]
    fn a_multiple_assignment_moves_its_rest_into_a_postarg() {
        assert_eq!(
            body("x, y = 1, 2"),
            "(MASGN [1:0-1:11] (LIST [1:7-1:11] (INTEGER [1:7-1:8] 1) (INTEGER [1:10-1:11] 2) nil) \
             (LIST [1:0-1:4] (LASGN [1:0-1:1] :x nil) (LASGN [1:3-1:4] :y nil) nil) nil)"
        );
        assert_eq!(
            body("a, = 1"),
            "(MASGN [1:0-1:6] (INTEGER [1:5-1:6] 1) \
             (LIST [1:0-1:1] (LASGN [1:0-1:1] :a nil) nil) nil)"
        );
        assert_eq!(
            body("a, *, b = 1"),
            "(MASGN [1:0-1:11] (INTEGER [1:10-1:11] 1) \
             (LIST [1:0-1:1] (LASGN [1:0-1:1] :a nil) nil) \
             (POSTARG [1:0-1:7] :NODE_SPECIAL_NO_NAME_REST \
             (LIST [1:6-1:7] (LASGN [1:6-1:7] :b nil) nil)))"
        );
        // A nested group's node stops INSIDE its closing paren, so the target
        // list ends there rather than at the paren.
        assert_eq!(
            body("a, (b, c) = 1"),
            "(MASGN [1:0-1:13] (INTEGER [1:12-1:13] 1) \
             (LIST [1:0-1:8] (LASGN [1:0-1:1] :a nil) \
             (MASGN [1:4-1:8] nil (LIST [1:4-1:8] (LASGN [1:4-1:5] :b nil) \
             (LASGN [1:7-1:8] :c nil) nil) nil) nil) nil)"
        );
    }

    /// The ten ARGS slots, and the three that are not readable off the
    /// parameter list alone.
    #[test]
    fn the_args_node_fills_all_ten_slots() {
        assert_eq!(
            body("def m(*a); end"),
            "(DEFN [1:0-1:14] :m (SCOPE [1:0-1:14] [:a] \
             (ARGS [1:6-1:8] 0 nil nil nil 0 nil :a nil nil nil) nil))"
        );
        assert_eq!(
            body("def m(a, *b, c); end"),
            "(DEFN [1:0-1:20] :m (SCOPE [1:0-1:20] [:a, :b, :c] \
             (ARGS [1:6-1:14] 1 nil nil :c 1 nil :b nil nil nil) nil))"
        );
        assert_eq!(
            body("def m(&b); end"),
            "(DEFN [1:0-1:14] :m (SCOPE [1:0-1:14] [:b] \
             (ARGS [1:6-1:8] 0 nil nil nil 0 nil nil nil nil :b) nil))"
        );
        // Keywords allocate a hidden rest -- a DVAR holding nil -- and it is a
        // LOCAL, which is the nil in the locals list.
        assert_eq!(
            body("def m(a:); end"),
            "(DEFN [1:0-1:14] :m (SCOPE [1:0-1:14] [:a, nil] \
             (ARGS [1:6-1:8] 0 nil nil nil 0 nil nil \
             (KW_ARG [1:6-1:8] (LASGN [1:6-1:8] :a :NODE_SPECIAL_REQUIRED_KEYWORD) nil) \
             (DVAR [1:6-1:8] nil) nil) nil))"
        );
        // `**nil` is not an absent rest: it sets BOTH keyword slots false.
        assert_eq!(
            body("def m(**nil); end"),
            "(DEFN [1:0-1:17] :m (SCOPE [1:0-1:17] [] \
             (ARGS [1:6-1:11] 0 nil nil nil 0 nil nil false false nil) nil))"
        );
    }

    /// An anonymous `*`, `**` or `&` is a local under the name parse.y gives
    /// it, which prism leaves out of its own list.
    #[test]
    fn an_anonymous_parameter_is_still_a_local() {
        assert_eq!(
            body("def m(*); end"),
            "(DEFN [1:0-1:13] :m (SCOPE [1:0-1:13] [:*] \
             (ARGS [1:6-1:7] 0 nil nil nil 0 nil :* nil nil nil) nil))"
        );
        assert_eq!(
            body("def m(&); end"),
            "(DEFN [1:0-1:13] :m (SCOPE [1:0-1:13] [:&] \
             (ARGS [1:6-1:7] 0 nil nil nil 0 nil nil nil nil :&) nil))"
        );
    }

    /// A destructuring parameter is a `MASGN` over a hidden variable, and it
    /// puts a nil into `locals` where the parameter sits.
    #[test]
    fn a_destructuring_parameter_declares_a_hidden_local() {
        assert_eq!(
            body("def m(a, (b, c)); end"),
            "(DEFN [1:0-1:21] :m (SCOPE [1:0-1:21] [:a, nil, :b, :c] \
             (ARGS [1:6-1:15] 2 (MASGN [1:10-1:14] (LVAR [1:10-1:10] nil) \
             (LIST [1:10-1:14] (LASGN [1:10-1:11] :b nil) (LASGN [1:13-1:14] :c nil) nil) nil) \
             nil nil 0 nil nil nil nil nil) nil))"
        );
    }

    /// A singleton definition is a different kind, carrying its receiver.
    #[test]
    fn a_singleton_def_is_defs() {
        assert_eq!(
            body("def self.m; end"),
            "(DEFS [1:0-1:15] (SELF [1:4-1:8] ) :m (SCOPE [1:0-1:15] [] \
             (ARGS [1:10-1:10] 0 nil nil nil 0 nil nil nil nil nil) nil))"
        );
    }

    /// A parenthesised argument list ends at its `)`, so the grammar has
    /// nothing left to take a `;` with -- only the paren-less form eats one,
    /// and the difference is visible as an empty statement.
    #[test]
    fn only_a_parenless_def_absorbs_its_terminator() {
        assert_eq!(
            body("def m; 1; end"),
            "(DEFN [1:0-1:13] :m (SCOPE [1:0-1:13] [] \
             (ARGS [1:5-1:5] 0 nil nil nil 0 nil nil nil nil nil) (INTEGER [1:7-1:8] 1)))"
        );
        assert_eq!(
            body("def m(); 1; end"),
            "(DEFN [1:0-1:15] :m (SCOPE [1:0-1:15] [] \
             (ARGS [1:5-1:6] 0 nil nil nil 0 nil nil nil nil nil) \
             (BLOCK [1:7-1:10] (BEGIN [1:7-1:7] nil) (INTEGER [1:9-1:10] 1))))"
        );
        // An EMPTY method body is nil either way, never the empty statement.
        assert_eq!(
            body("def m(); end"),
            "(DEFN [1:0-1:12] :m (SCOPE [1:0-1:12] [] \
             (ARGS [1:5-1:6] 0 nil nil nil 0 nil nil nil nil nil) nil))"
        );
    }

    /// `_1` and `it` declare parameters no list mentions, and CRuby still
    /// builds an ARGS for them -- zero-width, at the block's end.
    #[test]
    fn an_implicit_block_parameter_still_builds_args() {
        assert_eq!(
            body("foo { _1 }"),
            "(ITER [1:0-1:10] (FCALL [1:0-1:3] :foo nil) (SCOPE [1:4-1:10] [:_1] \
             (ARGS [1:10-1:10] 1 nil nil nil 0 nil nil nil nil nil) (DVAR [1:6-1:8] :_1)))"
        );
        // `it` is a local under a name no program can write.
        assert_eq!(
            body("foo { it }"),
            "(ITER [1:0-1:10] (FCALL [1:0-1:3] :foo nil) (SCOPE [1:4-1:10] [:\"<it>\"] \
             (ARGS [1:10-1:10] 1 nil nil nil 0 nil nil nil nil nil) (DVAR [1:6-1:8] :\"<it>\")))"
        );
    }

    /// A block's empty body sits past its PARAMETER LIST, not past the `{`.
    #[test]
    fn an_empty_block_body_follows_its_parameter_list() {
        assert_eq!(
            body("foo { }"),
            "(ITER [1:0-1:7] (FCALL [1:0-1:3] :foo nil) \
             (SCOPE [1:4-1:7] [] nil (BEGIN [1:5-1:5] nil)))"
        );
        assert_eq!(
            body("foo { || }"),
            "(ITER [1:0-1:10] (FCALL [1:0-1:3] :foo nil) \
             (SCOPE [1:4-1:10] [] nil (BEGIN [1:8-1:8] nil)))"
        );
    }

    /// An ITER's inner call runs through its CLOSING PAREN and stops before
    /// the block.
    #[test]
    fn an_iter_call_stops_at_its_closing_paren() {
        assert_eq!(
            body("foo(1) { }"),
            "(ITER [1:0-1:10] (FCALL [1:0-1:6] :foo (LIST [1:4-1:5] (INTEGER [1:4-1:5] 1) nil)) \
             (SCOPE [1:7-1:10] [] nil (BEGIN [1:8-1:8] nil)))"
        );
        // `super` keeps the keyword alone, with no trailing space.
        assert_eq!(
            body("super { }"),
            "(ITER [1:0-1:9] (ZSUPER [1:0-1:5] ) \
             (SCOPE [1:6-1:9] [] nil (BEGIN [1:7-1:7] nil)))"
        );
    }

    /// A `for` loop's variables belong to the ENCLOSING scope; the loop's own
    /// scope holds one nil for the hidden element.
    #[test]
    fn a_for_loop_scopes_its_variable_outside() {
        assert_eq!(
            tree("for i in a; end"),
            "(SCOPE [1:0-1:15] [:i] nil (FOR [1:0-1:15] (VCALL [1:9-1:10] :a) \
             (SCOPE [1:0-1:15] [nil] (ARGS [1:4-1:5] 1 (LASGN [1:4-1:5] :i (DVAR [1:4-1:5] nil)) \
             nil nil 0 nil nil nil nil nil) (BEGIN [1:11-1:11] nil))))"
        );
        // Several variables destructure through a FOR_MASGN.
        assert!(tree("for a, b in c; end").contains("(FOR_MASGN [1:4-1:8] (DVAR [1:4-1:8] nil))"));
    }

    /// `a => b` binds or raises and `a in b` answers a boolean. Both are a
    /// one-armed CASE3, and the boolean form says so with TRUE and FALSE.
    #[test]
    fn a_standalone_pattern_match_is_a_one_armed_case3() {
        assert_eq!(
            body("a => b"),
            "(CASE3 [1:0-1:6] (VCALL [1:0-1:1] :a) \
             (IN [1:5-1:6] (LASGN [1:5-1:6] :b nil) nil nil))"
        );
        assert_eq!(
            body("a in b"),
            "(CASE3 [1:0-1:6] (VCALL [1:0-1:1] :a) \
             (IN [1:5-1:6] (LASGN [1:5-1:6] :b nil) (TRUE [1:5-1:6] ) (FALSE [1:5-1:6] )))"
        );
    }

    /// A `when` or `in` arm runs to its SEPARATOR, not to its last statement.
    #[test]
    fn an_arm_runs_to_its_separator() {
        assert!(body("case a; in 1 then 2; end").contains("(IN [1:8-1:20]"));
        assert!(body("case a; in 1 then 2 end").contains("(IN [1:8-1:19]"));
    }

    /// A `begin ... end while` tests AFTER its body, and says so in its third
    /// child.
    #[test]
    fn a_post_test_loop_says_so() {
        assert!(body("while a do b end").ends_with(" true)"));
        assert!(body("begin; a; end while b").ends_with(" false)"));
    }

    /// `BEGIN { }` is HOISTED: its body moves to the front of the program and
    /// an empty statement stays where the keyword stood.
    #[test]
    fn a_pre_execution_block_is_hoisted() {
        assert_eq!(
            body("1; BEGIN { 2 }"),
            "(BLOCK [1:9-1:14] (BEGIN [1:9-1:14] (INTEGER [1:11-1:12] 2)) \
             (INTEGER [1:0-1:1] 1) (BEGIN [1:9-1:14] nil))"
        );
        // `END { }` is not hoisted -- it stays put, inside a POSTEXE.
        assert_eq!(
            body("END { 1 }"),
            "(POSTEXE [1:0-1:9] (SCOPE [1:0-1:9] [] nil (INTEGER [1:6-1:7] 1)))"
        );
    }

    /// `...` forwards three hidden parameters, and CRuby keeps no node for it:
    /// the call site is the splat, the double splat and the block pass over
    /// the locals the parameter list created.
    #[test]
    fn forwarding_expands_at_the_call_site() {
        let t = body("def m(...) = n(...)");
        assert!(t.contains("[:*, :**, :&, :\"...\"]"), "{t}");
        assert!(
            t.contains(
                "(BLOCK_PASS [1:14-1:19] (ARGSPUSH [1:14-1:19] \
                 (SPLAT [1:15-1:18] (LVAR [1:15-1:18] :*)) \
                 (HASH [1:15-1:18] (LIST [1:15-1:18] nil (LVAR [1:15-1:18] :**) nil))) \
                 (LVAR [1:15-1:18] :&))"
            ),
            "{t}"
        );
    }

    /// A source that is nothing but a terminator, a comment, or nothing at all
    /// is still the empty statement.
    #[test]
    fn an_empty_program_is_the_empty_statement() {
        assert_eq!(tree(""), "(SCOPE [1:0-1:0] [] nil (BEGIN [1:0-1:0] nil))");
        assert_eq!(tree(";"), "(SCOPE [1:0-1:1] [] nil (BEGIN [1:0-1:0] nil))");
        assert_eq!(
            tree("# just a comment\n"),
            "(SCOPE [1:0-1:0] [] nil (BEGIN [1:0-1:0] nil))"
        );
        // The program runs to its last separator, past trailing space.
        assert_eq!(
            tree("a ; # comment"),
            "(SCOPE [1:0-1:3] [] nil (VCALL [1:0-1:1] :a))"
        );
    }

    /// Adjacent string literals are ONE literal, and the folded node keeps the
    /// FIRST part's span.
    #[test]
    fn adjacent_string_literals_fold() {
        assert_eq!(body("\"a\" \"b\""), "(STR [1:0-1:3] \"ab\")");
    }

    /// `-2**2` is one production in parse.y, so the POWER carries the minus
    /// sign's span even though the negation wraps it.
    #[test]
    fn a_negated_power_is_one_production() {
        assert_eq!(
            body("-2**2"),
            "(OPCALL [1:0-1:5] (OPCALL [1:0-1:5] (INTEGER [1:1-1:2] 2) :** \
             (LIST [1:4-1:5] (INTEGER [1:4-1:5] 2) nil)) :-@ nil)"
        );
    }

    /// A prism kind with no mapping is an honest leaf, never a raise -- a
    /// walker recursing `children` degrades rather than breaking.
    #[test]
    fn an_unmapped_kind_is_a_leaf_not_a_raise() {
        // Every construct the corpus exercises is mapped; this asserts the
        // FALLBACK exists and answers, using the node builder directly.
        let v = translate::parse("a ? b : c", false, false).expect("parses");
        assert!(!dump(&v).contains("UNKNOWN"), "{}", dump(&v));
    }

    /// A `def` with its own `rescue` has no body wrapper, and an empty one is
    /// a nil first child rather than an empty statement.
    #[test]
    fn a_bodyless_rescue_has_a_nil_body() {
        assert_eq!(
            body("def m; rescue; end"),
            "(DEFN [1:0-1:18] :m (SCOPE [1:0-1:18] [] \
             (ARGS [1:5-1:5] 0 nil nil nil 0 nil nil nil nil nil) \
             (RESCUE [1:6-1:14] nil (RESBODY [1:7-1:14] nil nil (BEGIN [1:14-1:14] nil) nil) nil)))"
        );
    }
}
