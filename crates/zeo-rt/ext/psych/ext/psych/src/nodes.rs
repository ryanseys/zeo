//! The parse tree psych calls `Psych::Nodes::*`, and the one intermediate
//! form every load goes through.
//!
//! # Why the tree exists at all
//!
//! Building Ruby values straight out of yaml-rust2's event stream works and
//! is fast, but it cannot answer `Psych.parse`, which hands a caller the
//! DOCUMENT rather than the data -- and a gem that walks the tree to read a
//! tag, rewrite a scalar and emit it again has no other way in.
//!
//! So the event stream builds a [`Node`] tree, and everything else reads
//! THAT: `Psych.load` walks it into Ruby values, `Psych.parse` mirrors it
//! into `Psych::Nodes::*` objects, and `Nodes::Node#to_ruby` reads those
//! objects back into a `Node` and walks the same walk. One tree, one walk,
//! three entry points -- rather than two implementations of the same rules
//! that can drift.
//!
//! # The anchor names are recovered, not reported
//!
//! yaml-rust2's events carry an anchor as a NUMBER: `Scalar(v, style, 3,
//! tag)` and `Alias(3)`. Psych's nodes carry the name a human wrote, and a
//! program reads `node.anchor` expecting `"base"` rather than `3`.
//!
//! The parser assigns those ids by counting anchors as it meets them,
//! starting at 1 and never resetting across documents. So a pre-pass over
//! the SCANNER's token stream -- the same tokens the parser is about to read,
//! in the same order -- collects the names in that order, and the k-th one is
//! id k. See [`prescan`].
//!
//! # Marks
//!
//! Every node carries psych's `start_line`/`start_column`/`end_line`/
//! `end_column`. An event gives only where its token starts; the ends, and
//! the starts of nodes whose anchor or tag comes first, are read off the
//! source by libyaml's rules.

use std::collections::HashMap;

use yaml_rust2::parser::{Event, MarkedEventReceiver, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle, Token, TokenType};

/// Where a node starts and ends, as psych reports it: 0-based line and
/// column, the column counted in characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Marks {
    pub(super) start: (usize, usize),
    pub(super) end: (usize, usize),
}

/// Psych's `Psych::Nodes::Scalar::ANY` and its neighbours, and the same
/// numbering for a container's `BLOCK`/`FLOW`.
pub(super) mod style {
    pub const ANY: i64 = 0;
    pub const PLAIN: i64 = 1;
    pub const SINGLE_QUOTED: i64 = 2;
    pub const DOUBLE_QUOTED: i64 = 3;
    pub const LITERAL: i64 = 4;
    pub const FOLDED: i64 = 5;
    /// A container's styles, which share the 1/2 slots with a scalar's.
    pub const BLOCK: i64 = 1;
    pub const FLOW: i64 = 2;
}

/// One node of a parsed document.
#[derive(Clone, Debug)]
pub(super) enum Node {
    Scalar {
        value: String,
        /// yaml-rust2's style, kept as psych's number.
        style: i64,
        /// Whether the text was WRITTEN rather than spelled: quoted, or a
        /// literal or folded block. This is the flag the load walk reads, and
        /// not the style -- a scalar node built by hand carries a style of
        /// `ANY` and still has to be scanned, so `style != PLAIN` is the
        /// wrong question. Psych asks `quoted` for the same reason.
        quoted: bool,
        tag: Option<String>,
        anchor: Option<String>,
        mark: Marks,
    },
    Sequence {
        children: Vec<Node>,
        style: i64,
        tag: Option<String>,
        anchor: Option<String>,
        mark: Marks,
    },
    /// `children` alternates key, value, key, value -- which is psych's own
    /// shape and the reason a mapping node can hold an odd count when the
    /// document is malformed.
    Mapping {
        children: Vec<Node>,
        style: i64,
        tag: Option<String>,
        anchor: Option<String>,
        mark: Marks,
    },
    Alias {
        anchor: String,
        mark: Marks,
    },
}

impl Node {
    pub(super) fn anchor(&self) -> Option<&str> {
        match self {
            Node::Scalar { anchor, .. }
            | Node::Sequence { anchor, .. }
            | Node::Mapping { anchor, .. } => anchor.as_deref(),
            Node::Alias { anchor, .. } => Some(anchor),
        }
    }

    pub(super) fn marks(&self) -> Marks {
        match self {
            Node::Scalar { mark, .. }
            | Node::Sequence { mark, .. }
            | Node::Mapping { mark, .. }
            | Node::Alias { mark, .. } => *mark,
        }
    }
}

/// One document: its root, and what the header and footer said.
#[derive(Clone, Debug)]
pub(super) struct Document {
    pub(super) root: Option<Node>,
    /// `true` when the document had no `---` of its own.
    pub(super) implicit: bool,
    /// `true` when it had no `...` of its own.
    pub(super) implicit_end: bool,
    /// `%YAML 1.1` as `[major, minor]`, empty when the document names none.
    pub(super) version: Option<(i64, i64)>,
    /// `%TAG !foo! bar` pairs, in the order they were written.
    pub(super) tag_directives: Vec<(String, String)>,
    pub(super) mark: Marks,
}

const KIND_SCALAR: u8 = 0;
const KIND_SEQUENCE: u8 = 1;
const KIND_MAPPING: u8 = 2;

/// What a second scanner pass over the text recovers that the events do not
/// carry.
///
/// `names` holds every anchor's name in the order the parser numbers them:
/// index `i` is the parser's anchor id `i + 1`. `props` maps the content
/// token that follows an anchor or tag -- by character index and node kind --
/// to where that first property starts, because psych's node starts there
/// and the event marks the content. An empty scalar with a property has no
/// content token, so `empty_props` keys it by the next token instead.
///
/// A scan that fails answers what it collected before failing. It cannot
/// matter: the parse that follows reads the same tokens and fails too, so no
/// tree is ever built from a short scan.
#[derive(Default)]
pub(super) struct Prescan {
    names: Vec<String>,
    props: HashMap<(usize, u8), (usize, usize)>,
    empty_props: HashMap<usize, (usize, usize)>,
}

pub(super) fn prescan(src: &str) -> Prescan {
    let mut out = Prescan::default();
    let mut pending: Option<(usize, usize)> = None;
    for Token(mark, token) in yaml_rust2::scanner::Scanner::new(src.chars()) {
        let kind = match token {
            TokenType::Anchor(name) => {
                out.names.push(name);
                pending.get_or_insert(pos_of(mark));
                continue;
            }
            TokenType::Tag(..) => {
                pending.get_or_insert(pos_of(mark));
                continue;
            }
            TokenType::Scalar(..) => Some(KIND_SCALAR),
            TokenType::BlockSequenceStart
            | TokenType::FlowSequenceStart
            | TokenType::BlockEntry => Some(KIND_SEQUENCE),
            TokenType::BlockMappingStart | TokenType::FlowMappingStart => Some(KIND_MAPPING),
            _ => None,
        };
        if let Some(start) = pending.take() {
            match kind {
                Some(k) => out.props.insert((mark.index(), k), start),
                None => out.empty_props.insert(mark.index(), start),
            };
        }
    }
    out
}

/// A marker as psych counts it: yaml-rust2's lines start at 1.
fn pos_of(mark: Marker) -> (usize, usize) {
    (mark.line().saturating_sub(1), mark.col())
}

/// The line and column of byte `to`, counted from byte `from_byte`, which is
/// known to sit at `from`. Walking from a nearby known point keeps each
/// answer local rather than a count from the top of the text.
fn pos_rel(src: &str, from_byte: usize, from: (usize, usize), to: usize) -> (usize, usize) {
    let to = to.min(src.len());
    if to >= from_byte {
        let (mut line, mut col) = from;
        for c in src[from_byte..to].chars() {
            if c == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    } else {
        let breaks = src[to..from_byte].matches('\n').count();
        let line_start = src[..to].rfind('\n').map_or(0, |i| i + 1);
        (from.0.saturating_sub(breaks), src[line_start..to].chars().count())
    }
}

/// The byte where a scalar's text ends, read from the byte its token starts
/// at: after a quoted scalar's closing quote, after a plain scalar's last
/// character, and after a block scalar's lines.
fn scalar_end(src: &str, at: usize, sty: TScalarStyle, value: &str) -> usize {
    let at = at.min(src.len());
    let text = &src[at..];
    match sty {
        TScalarStyle::SingleQuoted => {
            let mut it = text.char_indices().skip(1).peekable();
            while let Some((i, c)) = it.next() {
                if c == '\'' {
                    // `''` is an escaped quote, not the end.
                    if it.peek().is_some_and(|&(_, n)| n == '\'') {
                        it.next();
                        continue;
                    }
                    return at + i + 1;
                }
            }
            src.len()
        }
        TScalarStyle::DoubleQuoted => {
            let mut it = text.char_indices().skip(1);
            while let Some((i, c)) = it.next() {
                match c {
                    '\\' => {
                        it.next();
                    }
                    '"' => return at + i + 1,
                    _ => {}
                }
            }
            src.len()
        }
        TScalarStyle::Literal | TScalarStyle::Folded => block_scalar_end(src, at),
        TScalarStyle::Plain => {
            // A plain scalar has no escapes: its value is its text with each
            // run of breaks and indentation folded, so matching the value
            // against the text finds the last character.
            let mut end = at;
            let mut chars = text.char_indices().peekable();
            for v in value.chars() {
                if v == ' ' || v == '\n' {
                    while chars
                        .peek()
                        .is_some_and(|&(_, c)| matches!(c, ' ' | '\t' | '\n' | '\r'))
                    {
                        chars.next();
                    }
                    continue;
                }
                match chars.next() {
                    Some((i, c)) if c == v => end = at + i + c.len_utf8(),
                    _ => break,
                }
            }
            end
        }
    }
}

/// Where a `|` or `>` block ends: at the start of the first line after the
/// header whose text is indented less than the block's first text line.
/// Blank lines before it belong to the block, as libyaml reads them.
fn block_scalar_end(src: &str, at: usize) -> usize {
    let Some(nl) = src[at..].find('\n') else {
        return src.len();
    };
    let mut pos = at + nl + 1;
    let mut indent = None;
    while pos < src.len() {
        let line_end = src[pos..].find('\n').map_or(src.len(), |i| pos + i);
        let line = &src[pos..line_end];
        if !line.trim().is_empty() {
            let lead = line.len() - line.trim_start_matches(' ').len();
            match indent {
                None if lead == 0 => break,
                None => indent = Some(lead),
                Some(n) if lead < n => break,
                Some(_) => {}
            }
        }
        pos = (line_end + 1).min(src.len());
    }
    pos
}

/// The byte of the `-` in front of `at`, past blanks, or `at` when there is
/// none.
fn dash_before(src: &str, at: usize) -> usize {
    let head = src[..at.min(src.len())].trim_end_matches([' ', '\t']);
    match head.ends_with('-') {
        true => head.len() - 1,
        false => at,
    }
}

/// The byte of the `|` or `>` that opens the block scalar whose text starts
/// at `at`: the last indicator on the last line of text before it.
fn block_indicator_before(src: &str, at: usize) -> usize {
    let end = after_content_before(src, at);
    let line_start = src[..end].rfind('\n').map_or(0, |i| i + 1);
    src[line_start..end]
        .rfind(['|', '>'])
        .map_or(at, |i| line_start + i)
}

/// The byte just past the last text before `at`, skipping blanks, breaks and
/// comments. libyaml puts an empty scalar there: right after the `:` or `-`
/// (or the property) that introduced it.
fn after_content_before(src: &str, at: usize) -> usize {
    let mut end = at.min(src.len());
    loop {
        let line_start = src[..end].rfind('\n').map_or(0, |i| i + 1);
        let seg = &src[line_start..end];
        let seg = match seg
            .char_indices()
            .find(|&(i, c)| c == '#' && (i == 0 || seg[..i].ends_with([' ', '\t'])))
        {
            Some((i, _)) => &seg[..i],
            None => seg,
        };
        let kept = seg.trim_end();
        if !kept.is_empty() || line_start == 0 {
            return line_start + kept.len();
        }
        end = line_start - 1;
    }
}

/// Builds a [`Document`] list out of the event stream.
pub(super) struct TreeBuilder<'a> {
    docs: Vec<Document>,
    /// The containers currently open, innermost last.
    stack: Vec<Node>,
    /// The root of the document being read, once it is complete.
    root: Option<Node>,
    /// Anchor names and property starts, when the caller asked for them.
    pre: Option<&'a Prescan>,
    src: &'a str,
    /// The BYTE offset where the open document started, for reading `---`
    /// back off the text.
    doc_start: usize,
    /// Where the open document starts, and where the stream ended.
    doc_mark: (usize, usize),
    stream_end: (usize, usize),
    /// A marker's index counts CHARACTERS, and slicing `src` needs bytes.
    /// The two agree only for ASCII, and disagreeing is a panic rather than a
    /// wrong answer: `&src[1..]` inside a two-byte character does not
    /// compile away, it aborts.
    ///
    /// Markers arrive in non-decreasing order through a parse, so one cursor
    /// walking forward converts every one of them for the cost of a single
    /// pass -- rather than a `char_indices().nth()` per container, which is
    /// quadratic on a large document.
    cursor: (usize, usize),
    version: Option<(i64, i64)>,
    tag_directives: Vec<(String, String)>,
}

impl<'a> TreeBuilder<'a> {
    /// `pre` empty means "do not resolve anchor names or property starts".
    pub(super) fn new(src: &'a str, pre: Option<&'a Prescan>) -> TreeBuilder<'a> {
        TreeBuilder {
            docs: Vec::new(),
            stack: Vec::new(),
            root: None,
            pre,
            src,
            doc_start: 0,
            doc_mark: (0, 0),
            stream_end: (0, 0),
            cursor: (0, 0),
            version: None,
            tag_directives: Vec::new(),
        }
    }

    /// The documents, and where the stream ended.
    pub(super) fn finish(self) -> (Vec<Document>, (usize, usize)) {
        (self.docs, self.stream_end)
    }

    fn name_of(&self, id: usize) -> Option<String> {
        if id == 0 {
            return None;
        }
        self.pre?.names.get(id - 1).cloned()
    }

    /// Where the anchor or tag in front of a node's content starts, if one
    /// does.
    fn prop_start(&self, index: usize, kind: u8) -> Option<(usize, usize)> {
        self.pre?.props.get(&(index, kind)).copied()
    }

    /// A finished node: into the open container, or -- with none open -- it
    /// is this document's root.
    fn place(&mut self, node: Node) {
        match self.stack.last_mut() {
            Some(Node::Sequence { children, .. } | Node::Mapping { children, .. }) => {
                children.push(node);
            }
            // A parser cannot open a scalar or an alias, so the remaining
            // arms are unreachable; taking the node as the root is what a
            // malformed stream would want anyway.
            _ => self.root = Some(node),
        }
    }

    fn pop_container(&mut self) {
        if let Some(node) = self.stack.pop() {
            self.place(node);
        }
    }

    /// The BYTE offset of a marker's character index.
    ///
    /// Walks forward from the last answer, and restarts from the beginning
    /// for the rare backward step -- which only happens when the parser
    /// re-reports a position it has already passed.
    fn byte_of(&mut self, chars: usize) -> usize {
        let (mut at_char, mut at_byte) = self.cursor;
        if chars < at_char {
            (at_char, at_byte) = (0, 0);
        }
        for c in self.src[at_byte..].chars() {
            if at_char >= chars {
                break;
            }
            at_char += 1;
            at_byte += c.len_utf8();
        }
        self.cursor = (at_char, at_byte);
        at_byte
    }

    /// Whether the document starting at byte `at` wrote its own `---`.
    ///
    /// yaml-rust2 reports `DocumentStart` for both kinds and does not say
    /// which, so the text is what answers. The marker sits at the first
    /// content, which for an explicit document is the `---` itself.
    fn explicit_at(&self, at: usize) -> bool {
        let at = at.min(self.src.len());
        self.src[..at]
            .lines()
            .next_back()
            .is_some_and(|l| l.trim_start().starts_with("---"))
            || self.src[at..].starts_with("---")
    }
}

impl MarkedEventReceiver for TreeBuilder<'_> {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        let pos = pos_of(mark);
        match ev {
            Event::DocumentStart => {
                self.root = None;
                self.stack.clear();
                self.doc_start = self.byte_of(mark.index());
                self.doc_mark = pos;
            }
            Event::DocumentEnd => {
                let implicit = !self.explicit_at(self.doc_start);
                // yaml-rust2 does not report a `...` either. Reading it back
                // off the text is the same trick as `---`, and the marker
                // here sits at the end of the document.
                let end = self.byte_of(mark.index()).min(self.src.len());
                let implicit_end = !self.src[end..].trim_start().starts_with("...");
                // An explicit end runs past its three dots.
                let end_mark = if implicit_end { pos } else { (pos.0, pos.1 + 3) };
                // An implicit document starts where its root does.
                let start = match &self.root {
                    Some(root) => self.doc_mark.min(root.marks().start),
                    None => self.doc_mark,
                };
                self.docs.push(Document {
                    root: self.root.take(),
                    implicit,
                    implicit_end,
                    version: self.version.take(),
                    tag_directives: std::mem::take(&mut self.tag_directives),
                    mark: Marks {
                        start,
                        end: end_mark,
                    },
                });
            }
            Event::Scalar(value, style, anchor, tag) => {
                let at = self.byte_of(mark.index());
                let mark = if value.is_empty() && style == TScalarStyle::Plain {
                    // An empty scalar has no text of its own. In a flow
                    // collection it sits at the token after it; in a block one
                    // where the `:` or `-` in front of it ends, or it spans its
                    // property. In a block sequence yaml-rust2 marks the next
                    // entry past its own `-`, so the walk back starts there.
                    let (flow, block_seq) = match self.stack.last() {
                        Some(Node::Sequence { style: s, .. }) => (*s == style::FLOW, *s != style::FLOW),
                        Some(Node::Mapping { style: s, .. }) => (*s == style::FLOW, false),
                        _ => (false, false),
                    };
                    let from = if block_seq { dash_before(self.src, at) } else { at };
                    // yaml-rust2 marks an empty flow VALUE at its own `:`,
                    // and libyaml at the token after it.
                    let flow_value = flow
                        && self.src[at..].starts_with(':')
                        && matches!(self.stack.last(), Some(Node::Mapping { children, .. }) if children.len() % 2 == 1);
                    let end = match (flow, flow_value) {
                        (true, true) => {
                            let rest = &self.src[at + 1..];
                            let blanks = rest.len() - rest.trim_start_matches([' ', '\t', '\n', '\r']).len();
                            pos_rel(self.src, at, pos, at + 1 + blanks)
                        }
                        (true, false) => pos,
                        (false, _) => pos_rel(self.src, at, pos, after_content_before(self.src, from)),
                    };
                    let start = match anchor != 0 || tag.is_some() {
                        true => self
                            .pre
                            .and_then(|p| p.empty_props.get(&mark.index()).copied())
                            .unwrap_or(end),
                        false => end,
                    };
                    Marks { start, end }
                } else {
                    // yaml-rust2 marks a block scalar's first line of text;
                    // the node starts at its `|` or `>`.
                    let head = match style {
                        TScalarStyle::Literal | TScalarStyle::Folded => {
                            block_indicator_before(self.src, at)
                        }
                        _ => at,
                    };
                    let end = scalar_end(self.src, head, style, &value);
                    Marks {
                        start: self
                            .prop_start(mark.index(), KIND_SCALAR)
                            .unwrap_or_else(|| pos_rel(self.src, at, pos, head)),
                        end: pos_rel(self.src, at, pos, end),
                    }
                };
                let node = Node::Scalar {
                    value,
                    style: scalar_style(style),
                    quoted: style != TScalarStyle::Plain,
                    tag: tag.map(tag_text),
                    anchor: self.name_of(anchor),
                    mark,
                };
                self.place(node);
            }
            Event::SequenceStart(anchor, tag) => {
                let at = self.byte_of(mark.index());
                let sty = container_style(self.src, at);
                // An indentless sequence is marked past its first `- `; it
                // starts at the dash.
                let pos = match sty == style::BLOCK && !self.src[at..].starts_with('-') {
                    true => pos_rel(self.src, at, pos, dash_before(self.src, at)),
                    false => pos,
                };
                let start = self.prop_start(mark.index(), KIND_SEQUENCE).unwrap_or(pos);
                let node = Node::Sequence {
                    children: Vec::new(),
                    style: sty,
                    tag: tag.map(tag_text),
                    anchor: self.name_of(anchor),
                    mark: Marks { start, end: pos },
                };
                self.stack.push(node);
            }
            Event::MappingStart(anchor, tag) => {
                let at = self.byte_of(mark.index());
                let start = self.prop_start(mark.index(), KIND_MAPPING).unwrap_or(pos);
                let node = Node::Mapping {
                    children: Vec::new(),
                    style: container_style(self.src, at),
                    tag: tag.map(tag_text),
                    anchor: self.name_of(anchor),
                    mark: Marks { start, end: pos },
                };
                self.stack.push(node);
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some(
                    Node::Sequence {
                        style: sty,
                        mark,
                        children,
                        ..
                    }
                    | Node::Mapping {
                        style: sty,
                        mark,
                        children,
                        ..
                    },
                ) = self.stack.last_mut()
                {
                    // A flow container ends past its closing bracket; a block
                    // one where the next token begins.
                    mark.end = match *sty == style::FLOW {
                        true => (pos.0, pos.1 + 1),
                        false => pos,
                    };
                    // yaml-rust2 marks a block mapping at its first `:`; the
                    // node starts no later than its first key.
                    if let Some(first) = children.first() {
                        mark.start = mark.start.min(first.marks().start);
                    }
                }
                self.pop_container();
            }
            Event::Alias(id) => {
                // A name is only missing when the caller did not ask for
                // names, and then nothing reads it.
                let anchor = self.name_of(id).unwrap_or_default();
                let end = (pos.0, pos.1 + 1 + anchor.chars().count());
                self.place(Node::Alias {
                    anchor,
                    mark: Marks { start: pos, end },
                });
            }
            Event::StreamEnd => self.stream_end = pos,
            Event::Nothing | Event::StreamStart => {}
        }
    }
}

/// yaml-rust2 hands a tag as `(handle, suffix)`; psych compares the whole
/// name, so put it back together.
pub(super) fn tag_text(t: Tag) -> String {
    match t.handle.as_str() {
        "" => t.suffix,
        h => format!("{h}{}", t.suffix),
    }
}

fn scalar_style(s: TScalarStyle) -> i64 {
    match s {
        TScalarStyle::Plain => style::PLAIN,
        TScalarStyle::SingleQuoted => style::SINGLE_QUOTED,
        TScalarStyle::DoubleQuoted => style::DOUBLE_QUOTED,
        TScalarStyle::Literal => style::LITERAL,
        TScalarStyle::Folded => style::FOLDED,
    }
}

/// Block or flow, read off the text.
///
/// The event says only that a container opened. Psych's parser knows which
/// because libyaml tells it, and a program that re-emits a parsed tree needs
/// the answer or every flow mapping comes back as a block one. The first
/// non-space character at the container's start is `{` or `[` for a flow
/// container and anything else for a block one.
///
/// `at` is a BYTE offset -- see `TreeBuilder::byte_of`.
fn container_style(src: &str, at: usize) -> i64 {
    match src[at.min(src.len())..].trim_start().as_bytes().first() {
        Some(b'{' | b'[') => style::FLOW,
        Some(_) => style::BLOCK,
        // An empty container at the very end of the text: `[]` and `{}` both
        // have their brace before this point, so there is nothing to read.
        None => style::ANY,
    }
}

/// Version and tag directives, which yaml-rust2 consumes without reporting.
///
/// `%YAML` and `%TAG` reach the SCANNER as tokens and stop there: the parser
/// applies them and emits no event, so an event-only reader cannot see them
/// and `Nodes::Document#version` would always be empty. The same pre-pass
/// that recovers anchor names collects these.
pub(super) type Directives = (Option<(i64, i64)>, Vec<(String, String)>);
pub(super) fn directives(src: &str) -> Directives {
    let mut version = None;
    let mut tags = Vec::new();
    for token in yaml_rust2::scanner::Scanner::new(src.chars()) {
        match token.1 {
            TokenType::VersionDirective(major, minor) => {
                version = Some((i64::from(major), i64::from(minor)));
            }
            TokenType::TagDirective(handle, prefix) => tags.push((handle, prefix)),
            // Directives belong to the document they precede, and only the
            // first document's are read here -- see `parse_tree`.
            TokenType::DocumentStart => break,
            _ => {}
        }
    }
    (version, tags)
}
