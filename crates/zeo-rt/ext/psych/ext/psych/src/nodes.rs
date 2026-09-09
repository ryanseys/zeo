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
//! id k. See [`anchor_names`].


use yaml_rust2::parser::{Event, MarkedEventReceiver, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle, TokenType};

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
    },
    Sequence {
        children: Vec<Node>,
        style: i64,
        tag: Option<String>,
        anchor: Option<String>,
    },
    /// `children` alternates key, value, key, value -- which is psych's own
    /// shape and the reason a mapping node can hold an odd count when the
    /// document is malformed.
    Mapping {
        children: Vec<Node>,
        style: i64,
        tag: Option<String>,
        anchor: Option<String>,
    },
    Alias {
        anchor: String,
    },
}

impl Node {
    pub(super) fn anchor(&self) -> Option<&str> {
        match self {
            Node::Scalar { anchor, .. }
            | Node::Sequence { anchor, .. }
            | Node::Mapping { anchor, .. } => anchor.as_deref(),
            Node::Alias { anchor } => Some(anchor),
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
}

/// The name of every anchor in `src`, in the order the parser will number
/// them: the answer's index `i` is the parser's anchor id `i + 1`.
///
/// The scanner is run a second time over the same text to get them. That is
/// one extra pass, and it happens ONLY for `Psych.parse` -- a `Psych.load`
/// never asks, because it resolves an alias by id and never needs the name.
///
/// A scan that fails answers what it collected before failing. It cannot
/// matter: the parse that follows reads the same tokens and fails too, so no
/// tree is ever built from a short list.
pub(super) fn anchor_names(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in yaml_rust2::scanner::Scanner::new(src.chars()) {
        if let TokenType::Anchor(name) = token.1 {
            out.push(name);
        }
    }
    out
}

/// Builds a [`Document`] list out of the event stream.
pub(super) struct TreeBuilder<'a> {
    docs: Vec<Document>,
    /// The containers currently open, innermost last.
    stack: Vec<Node>,
    /// The root of the document being read, once it is complete.
    root: Option<Node>,
    /// Anchor names by parser id, when the caller asked for them.
    names: Option<&'a [String]>,
    src: &'a str,
    /// The BYTE offset where the open document started, for reading `---`
    /// back off the text.
    doc_start: usize,
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
    /// `names` empty means "do not resolve anchor names", which is what a
    /// plain `Psych.load` passes -- it never reads one.
    pub(super) fn new(src: &'a str, names: Option<&'a [String]>) -> TreeBuilder<'a> {
        TreeBuilder {
            docs: Vec::new(),
            stack: Vec::new(),
            root: None,
            names,
            src,
            doc_start: 0,
            cursor: (0, 0),
            version: None,
            tag_directives: Vec::new(),
        }
    }

    pub(super) fn finish(self) -> Vec<Document> {
        self.docs
    }

    fn name_of(&self, id: usize) -> Option<String> {
        if id == 0 {
            return None;
        }
        self.names?.get(id - 1).cloned()
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
        match ev {
            Event::DocumentStart => {
                self.root = None;
                self.stack.clear();
                self.doc_start = self.byte_of(mark.index());
            }
            Event::DocumentEnd => {
                let implicit = !self.explicit_at(self.doc_start);
                // yaml-rust2 does not report a `...` either. Reading it back
                // off the text is the same trick as `---`, and the marker
                // here sits at the end of the document.
                let end = self.byte_of(mark.index()).min(self.src.len());
                let implicit_end = !self.src[end..].trim_start().starts_with("...");
                self.docs.push(Document {
                    root: self.root.take(),
                    implicit,
                    implicit_end,
                    version: self.version.take(),
                    tag_directives: std::mem::take(&mut self.tag_directives),
                });
            }
            Event::Scalar(value, style, anchor, tag) => {
                let node = Node::Scalar {
                    value,
                    style: scalar_style(style),
                    quoted: style != TScalarStyle::Plain,
                    tag: tag.map(tag_text),
                    anchor: self.name_of(anchor),
                };
                self.place(node);
            }
            Event::SequenceStart(anchor, tag) => {
                let at = self.byte_of(mark.index());
                let node = Node::Sequence {
                    children: Vec::new(),
                    style: container_style(self.src, at),
                    tag: tag.map(tag_text),
                    anchor: self.name_of(anchor),
                };
                self.stack.push(node);
            }
            Event::MappingStart(anchor, tag) => {
                let at = self.byte_of(mark.index());
                let node = Node::Mapping {
                    children: Vec::new(),
                    style: container_style(self.src, at),
                    tag: tag.map(tag_text),
                    anchor: self.name_of(anchor),
                };
                self.stack.push(node);
            }
            Event::SequenceEnd | Event::MappingEnd => self.pop_container(),
            Event::Alias(id) => {
                // A name is only missing when the caller did not ask for
                // names, and then nothing reads it.
                let anchor = self.name_of(id).unwrap_or_default();
                self.place(Node::Alias { anchor });
            }
            Event::Nothing | Event::StreamStart | Event::StreamEnd => {}
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
