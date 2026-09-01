//! `psych` / `yaml` (CRuby's bundled YAML engine). `require "psych"` activates
//! `Psych`; `require "yaml"` additionally exposes `YAML` (the same module --
//! Ruby's `yaml.rb` is just `YAML = Psych`). Both dispatch here.
//!
//! Loading drives `yaml-rust2`'s PARSER (the maintained pure-Rust YAML 1.2
//! engine) and puts psych's own semantics above it: [`scanner`] resolves a
//! plain scalar by YAML **1.1** rules, and [`loader`] reads the anchor id
//! and the tag every event already carried. Dumping is hand-rolled to
//! reproduce Psych's block style, which the yaml-rust2 emitter does not
//! match.
//!
//! This doc used to call four things "documented divergences". THREE OF
//! THEM WERE BUGS, and they are fixed: an alias resolved to `nil` (silent
//! data loss), a tag was ignored outright (a wrong type AND a defeated
//! `safe_load` gate), and a `<<` merge key stayed literal. A
//! `Psych::SyntaxError` now carries the six marks CRuby's does, and its
//! message is built the same way -- what still differs is the WORDING
//! yaml-rust2 and libyaml choose for the same complaint, and the position a
//! block-indentation error is attributed to.
//!
//! # One tree, three entry points
//!
//! Every load now goes through the [`nodes`] tree: the events build it,
//! [`loader`] walks it into Ruby values, and [`tree_api`] mirrors it into the
//! `Psych::Nodes::*` objects `Psych.parse` answers. `Nodes::Node#to_ruby`
//! reads those objects back and walks the same walk, so a hand-built tree and
//! a parsed one cannot disagree.
//!
//! A syntax error raises `Psych::SyntaxError`, which this gem's RUBY half
//! (`ext/psych/lib/psych.rb`) defines -- see `ext/json` for why the
//! exception lives there and not here.

mod emitter;
mod loader;
mod nodes;
mod scanner;
mod tree_api;

use crate::dispatch::raise_error;
use crate::{RubyValue, Signal, string_new};
use zeo_macros::ruby_module;


/// One option out of the trailing keyword Hash.
fn opt(opts: Option<&RubyValue>, name: &str) -> Option<RubyValue> {
    let RubyValue::Hash(h) = opts? else {
        return None;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    match crate::collections::hash_get(h, &key) {
        RubyValue::Nil => None,
        v => Some(v),
    }
}

fn bool_opt(opts: Option<&RubyValue>, name: &str) -> bool {
    opt(opts, name).is_some_and(|v| v.truthy())
}

/// What `safe_load` was asked for. `permitted_classes` names CLASSES; they
/// are compared by name, which is what lets `Psych::Set` be permitted
/// without the loader owning the class.
fn safe_opts(opts: Option<&RubyValue>) -> loader::LoadOpts {
    let permitted = match opt(opts, "permitted_classes") {
        Some(RubyValue::Array(a)) => a
            .lock()
            .iter()
            .map(crate::value::RubyValue::to_display_string)
            .collect(),
        Some(other) => vec![other.to_display_string()],
        None => Vec::new(),
    };
    loader::LoadOpts {
        permitted: Some(permitted),
        aliases: bool_opt(opts, "aliases"),
        symbolize_names: bool_opt(opts, "symbolize_names"),
        freeze: bool_opt(opts, "freeze"),
    }
}

/// The anchor name an "unknown anchor" error is about.
///
/// yaml-rust2 reports the BYTE the alias starts at and not the name, so
/// the name is read back out of the source -- which is what lets the
/// message name the anchor the way psych's does.
fn anchor_name_at(src: &str, err: &str) -> String {
    let Some(rest) = err.split("at byte ").nth(1) else {
        return String::new();
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let Ok(at) = digits.parse::<usize>() else {
        return String::new();
    };
    src.get(at..)
        .unwrap_or("")
        .trim_start_matches('*')
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, ',' | ']' | '}'))
        .collect()
}

/// What a dump was asked for.
fn dump_opts(opts: Option<&RubyValue>) -> emitter::DumpOpts {
    let usize_opt = |name: &str, default: usize| match opt(opts, name) {
        Some(RubyValue::Int(n)) if n > 0 => n as usize,
        _ => default,
    };
    emitter::DumpOpts {
        indentation: usize_opt("indentation", 2),
        line_width: usize_opt("line_width", 80),
        header: bool_opt(opts, "header"),
    }
}

/// What `load`, `load_file` and `load_stream` were asked for: `safe_load`'s
/// options, except that Symbol is permitted unless the caller named its own
/// list. That one difference is the whole of `load` versus `safe_load`.
fn load_opts(opts: Option<&RubyValue>) -> loader::LoadOpts {
    let mut o = safe_opts(opts);
    if opt(opts, "permitted_classes").is_none() {
        o.permitted = Some(vec!["Symbol".to_string()]);
    }
    o
}

/// Whether byte `at` sits inside an unclosed flow collection -- a `[` or `{`
/// with no partner before it, counted outside quotes and comments.
fn in_flow_context(text: &str, at: usize) -> bool {
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut comment = false;
    for &b in &text.as_bytes()[..at.min(text.len())] {
        match (quote, comment, b) {
            (_, _, b'\n') => comment = false,
            (Some(q), _, c) if c == q => quote = None,
            (Some(_), ..) | (_, true, _) => {}
            (None, false, b'"' | b'\'') => quote = Some(b),
            (None, false, b'#') => comment = true,
            (None, false, b'[' | b'{') => depth += 1,
            (None, false, b']' | b'}') => depth -= 1,
            _ => {}
        }
    }
    depth > 0
}

/// A `Psych::SyntaxError` carrying the marks CRuby's does.
///
/// Its six readers are the interface -- rubygems and bundler both report
/// `e.line`. The message is built here rather than in the Ruby half so that
/// the one `raise_error` path still names the class, and the ivars are set on
/// the raised object afterwards, which is what fills the readers.
///
/// yaml-rust2 writes the two halves as one `info` string, CONTEXT first
/// (`"while parsing a node, did not find expected node content"`), where
/// psych keeps them apart. The split is that comma. The context WORDING is
/// still yaml-rust2's, not libyaml's -- see the module docs.
fn syntax_error(text: &str, file: Option<&str>, err: &yaml_rust2::ScanError) -> Signal {
    let mark = err.marker();
    let (context, problem) = match err.info().split_once(", ") {
        Some((c, p)) if c.starts_with("while ") => (Some(c), p),
        _ => (None, err.info()),
    };
    // libyaml names the two node kinds apart and yaml-rust2 does not. Only
    // the FLOW half can be proved from the source -- an unclosed `[`/`{`
    // before the mark -- so only that half is renamed; the block case keeps
    // yaml-rust2's wording rather than risking a wrong claim.
    let flow_context = "while parsing a flow node";
    let context = match context {
        Some("while parsing a node") if in_flow_context(text, mark.index()) => Some(flow_context),
        other => other,
    };
    let joined = match context {
        Some(c) => format!("{problem} {c}"),
        None => problem.to_string(),
    };
    // yaml-rust2's `col` is 0-based despite its doc comment; psych's is 1-based.
    let column = mark.col() + 1;
    let sig = raise_error(
        "Psych::SyntaxError",
        format!(
            "({}): {joined} at line {} column {column}",
            file.unwrap_or("<unknown>"),
            mark.line(),
        ),
    );
    let Signal::Raise(exc) = &sig else {
        return sig;
    };
    let str_or_nil = |s: Option<&str>| match s {
        Some(s) => RubyValue::Str(string_new(s.to_string())),
        None => RubyValue::Nil,
    };
    let marks: [(&str, RubyValue); 6] = [
        ("file", str_or_nil(file)),
        ("line", RubyValue::Int(mark.line() as i64)),
        ("column", RubyValue::Int(column as i64)),
        ("offset", RubyValue::Int(mark.index() as i64)),
        ("problem", str_or_nil(Some(problem))),
        ("context", str_or_nil(context)),
    ];
    for (name, v) in marks {
        let _ = crate::dispatch::ivar_set_dyn(exc, name, v);
    }
    sig
}

/// Parse `text` into the node tree every entry point reads.
///
/// The anchor NAMES come from a second pass over the scanner -- see
/// [`nodes::anchor_names`] for why the events cannot supply them -- and the
/// directives from the same place, because the parser applies `%YAML` and
/// `%TAG` without reporting them.
fn parse_tree(
    text: &str,
    file: Option<&str>,
    aliases: bool,
) -> Result<Vec<nodes::Document>, Signal> {
    let names = nodes::anchor_names(text);
    let mut builder = nodes::TreeBuilder::new(text, Some(&names));
    yaml_rust2::parser::Parser::new_from_str(text)
        .load(&mut builder, true)
        .map_err(|e| {
            // yaml-rust2 refuses an undefined anchor while SCANNING, so the
            // walk's own `Alias` arm never sees it -- but psych names it, and
            // a program rescues that name.
            match (e.info().contains("unknown anchor"), aliases) {
                // With aliases OFF, ruby refuses the alias itself before it
                // ever asks what it points at, so an alias to an anchor that
                // does not exist reports the DIAL and not the anchor. zeo's
                // parser refuses first, which had it the other way round.
                (true, false) => raise_error(
                    "Psych::AliasesNotEnabled",
                    "Alias parsing was not enabled. To enable it, pass `aliases: true` to \
                     `Psych::load` or `Psych::safe_load`."
                        .to_string(),
                ),
                (true, true) => raise_error(
                    "Psych::AnchorNotDefined",
                    format!(
                        "An alias referenced an unknown anchor: {}",
                        anchor_name_at(text, &format!("{e}"))
                    ),
                ),
                (false, _) => syntax_error(text, file, &e),
            }
        })?;
    let mut docs = builder.finish();
    // The directives belong to the first document; a stream that gives each
    // document its own is rare enough that reading them per document would
    // cost a scan each for an answer nothing has asked for.
    if let Some(first) = docs.first_mut() {
        let (version, tags) = nodes::directives(text);
        first.version = version;
        first.tag_directives = tags;
    }
    Ok(docs)
}

/// Every document in `text`, as Ruby values.
fn load_documents(
    text: &str,
    file: Option<&str>,
    opts: &loader::LoadOpts,
) -> Result<Vec<RubyValue>, Signal> {
    let docs = parse_tree(text, file, opts.aliases)?;
    loader::Revive::new(opts).documents(&docs)
}

/// The `filename:` a load/parse entry was given -- what a `Psych::SyntaxError`
/// from it reports as its `file`.
fn file_opt(opts: Option<&RubyValue>) -> Option<String> {
    opt(opts, "filename").map(|v| v.to_display_string())
}

/// The FIRST document, which is what every `load` entry answers.
///
/// An EMPTY stream answers the FALLBACK, and the three entries disagree
/// about what that is: `load` and `safe_load` answer `nil` while
/// `unsafe_load` answers `false`. A caller can name its own with
/// `fallback:`. Probed -- it is not derivable from anything.
fn first_document(
    text: &str,
    file: Option<&str>,
    opts: &loader::LoadOpts,
    fallback: RubyValue,
) -> Result<RubyValue, Signal> {
    Ok(load_documents(text, file, opts)?
        .into_iter()
        .next()
        .unwrap_or(fallback))
}

fn load_text(v: &RubyValue) -> Result<String, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned())
}

// `YAML = Psych` -- ruby's `yaml.rb` is that one line, so the alias id
// carries Psych's class surface and no instance half of its own.
crate::alias_class_tables! {
    YAML_TABLE = zeo_abi::YAML_MODULE,
}

const fn alias_table(id: crate::ClassId) -> crate::builtins::BuiltinClassTable {
    crate::builtins::BuiltinClassTable {
        id,
        instance: None,
        class: Some(crate::builtins::MethodTable {
            lookup: lookup_class,
            names: lookup_class_names,
            arity: lookup_class_arity,
            params: lookup_class_params,
            is_private: lookup_class_is_private,
            is_protected: lookup_class_is_protected,
            allocs: lookup_class_allocs,
            inherits: lookup_class_inherits,
            gate: lookup_class_gate,
            has_gated: false,
        }),
        install_constants: None,
        // An alias never allocates: it is another class's table under a
        // second id, and the class it copies owns construction.
        allocate: None,
    }
}

ruby_module! {
    Psych = zeo_abi::PSYCH_MODULE;

    // CRuby's `Psych.load`/`dump`/... are singleton module methods (`def self.`),
    // each arity -2 (one required arg + optional opts).
    // `opts` is ignored.
    // The three entry points differ ONLY in what they permit: `safe_load`
    // permits what the caller passed, `load` permits Symbol and nothing
    // else, `unsafe_load` permits everything. Aliases are their own
    // switch, off for both `load` and `safe_load`.
    def self."safe_load" (_recv, yaml, **opts) {
        let text = load_text(yaml)?;
        let fallback = opt(opts.as_ref().copied(), "fallback").unwrap_or(RubyValue::Nil);
        first_document(&text, file_opt(opts.as_ref().copied()).as_deref(), &safe_opts(opts.as_ref().copied()), fallback)
    }
    def self."load" (_recv, yaml, **opts) {
        let text = load_text(yaml)?;
        let fallback = opt(opts.as_ref().copied(), "fallback").unwrap_or(RubyValue::Nil);
        first_document(&text, file_opt(opts.as_ref().copied()).as_deref(), &load_opts(opts.as_ref().copied()), fallback)
    }
    def self."unsafe_load" (_recv, yaml, **opts) {
        let text = load_text(yaml)?;
        let mut o = safe_opts(opts.as_ref().copied());
        o.permitted = None;
        o.aliases = true;
        let fallback =
            opt(opts.as_ref().copied(), "fallback").unwrap_or(RubyValue::Bool(false));
        first_document(&text, file_opt(opts.as_ref().copied()).as_deref(), &o, fallback)
    }
    // Only the compact string form; an `io`/`opts` argument is ignored.
    def self."dump" (_recv, obj, io?, options?) {
        // `dump(obj, io = nil, options = {})`, and the second argument is
        // overloaded: a Hash there IS the options, which is how
        // `YAML.dump(x, indentation: 4)` reaches them.
        let options = match (&io, &options) {
            (Some(RubyValue::Hash(_)), None) => io,
            _ => options,
        };
        let text = emitter::Emitter::new(&dump_opts(options.as_ref().copied())).dump(obj)?;
        let out = RubyValue::Str(string_new(text));
        match io {
            Some(port) if !matches!(port, RubyValue::Hash(_) | RubyValue::Nil) => {
                crate::dispatch::send_value(
                    port,
                    crate::Symbol::intern("write"),
                    &[out],
                    None,
                )?;
                Ok(port.clone())
            }
            _ => Ok(out),
        }
    }

    // `Psych.load_file(path)` -- read the file and load its first document.
    // `opts` is ignored.
    def self."load_file" (_recv, filename, **opts) {
        let path = crate::builtins::file::path_arg(filename, "load_file")?;
        let text = crate::gvl::without_gvl(|| std::fs::read_to_string(&path)).map_err(|e| raise_error(
            "Errno::ENOENT",
            format!("No such file or directory - {path} ({e})"),
        ))?;
        let fallback = opt(opts.as_ref().copied(), "fallback").unwrap_or(RubyValue::Nil);
        // The PATH is the filename a syntax error reports, unless the caller
        // named its own -- CRuby's `load_file` passes it the same way.
        let file = file_opt(opts.as_ref().copied()).unwrap_or_else(|| path.clone());
        first_document(&text, Some(&file), &load_opts(opts.as_ref().copied()), fallback)
    }
    // `Psych.load_stream(yaml)` -- EVERY document; an Array, or yielded one by
    // one to a block (then the receiver's nil, matching CRuby's block form).
    def self."load_stream" (_recv, arg1, **opts, &block) {
        let text = load_text(arg1)?;
        let docs = load_documents(&text, file_opt(opts.as_ref().copied()).as_deref(), &load_opts(opts.as_ref().copied()))?;
        if let Some(RubyValue::Proc(p)) = &block {
            for doc in &docs {
                p.call(std::slice::from_ref(doc))?;
            }
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Array(crate::array_new(docs)))
    }

    // `Psych.parse(yaml)` -- the FIRST document as a `Psych::Nodes::Document`,
    // or the `fallback:` (nil by default) for an empty stream. That is psych's
    // own answer: a document with no root would be a node a caller cannot use.
    def self."parse" (_recv, yaml, **opts) {
        let text = load_text(yaml)?;
        let docs = parse_tree(&text, file_opt(opts.as_ref().copied()).as_deref(), true)?;
        // An empty stream answers FALSE, not nil -- which is `parse`'s own
        // fallback and differs from `load`'s. Measured, not derived.
        let fallback =
            opt(opts.as_ref().copied(), "fallback").unwrap_or(RubyValue::Bool(false));
        Ok(tree_api::to_ruby_documents(&docs)?
            .into_iter()
            .next()
            .unwrap_or(fallback))
    }

    // `Psych.parse_stream(yaml)` -- the whole stream as a
    // `Psych::Nodes::Stream`, or each document yielded to a block (then nil,
    // matching `load_stream`'s block form and CRuby's).
    def self."parse_stream" (_recv, yaml, **opts, &block) {
        let _ = &opts;
        let text = load_text(yaml)?;
        let docs = parse_tree(&text, file_opt(opts.as_ref().copied()).as_deref(), true)?;
        if let Some(RubyValue::Proc(p)) = &block {
            for doc in tree_api::to_ruby_documents(&docs)? {
                p.call(&[doc])?;
            }
            return Ok(RubyValue::Nil);
        }
        tree_api::to_ruby_stream(&docs)
    }

    // `Psych::Nodes::Node#to_ruby`'s engine, called from the Ruby half rather
    // than by a program: it takes the node OBJECT, reads it back into the
    // parse tree and walks the one walk `load` walks.
    def self."__node_to_ruby" (_recv, node, **opts) {
        let mut o = load_opts(opts.as_ref().copied());
        // A node tree is already parsed, so there is nothing left to gate:
        // `to_ruby` on a tree the caller is holding is psych's UNSAFE entry,
        // and its `safe_load` half refuses at parse time instead.
        o.permitted = None;
        o.aliases = true;
        o.symbolize_names = bool_opt(opts.as_ref().copied(), "symbolize_names");
        o.freeze = bool_opt(opts.as_ref().copied(), "freeze");

        let wrap = |root| nodes::Document {
            root: Some(root),
            implicit: true,
            implicit_end: true,
            version: None,
            tag_directives: Vec::new(),
        };
        // A STREAM answers one value per document, as an Array. Every other
        // node answers the single value it describes.
        if tree_api::node_kind(node) == "Stream" {
            let docs: Vec<nodes::Document> =
                tree_api::stream_roots(node)?.into_iter().map(wrap).collect();
            let values = loader::Revive::new(&o).documents(&docs)?;
            return Ok(RubyValue::Array(crate::collections::array_new(values)));
        }
        let Some(tree) = tree_api::from_ruby_nodes(node)? else {
            return Ok(RubyValue::Nil);
        };
        Ok(loader::Revive::new(&o)
            .documents(std::slice::from_ref(&wrap(tree)))?
            .into_iter()
            .next()
            .unwrap_or(RubyValue::Nil))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{array_new, hash_new};

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    /// `Psych`'s `ruby_module!`-generated functions have mangled Rust idents, so
    /// the tests call them through the registered class-method `lookup`.
    fn f(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::PSYCH_MODULE)
            .expect("Psych is a registered builtin table")
            .class
            .as_ref()
            .expect("Psych has class methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Psych.{name} is defined"))
    }
    fn dumped(v: &RubyValue) -> String {
        match f("dump")(&RubyValue::Nil, std::slice::from_ref(v), None).unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn load_maps_and_sequences() {
        let v = f("load")(
            &RubyValue::Nil,
            &[s("a: 1\nb:\n  - 2\n  - 3.5\n  - null\n  - true")],
            None,
        )
        .unwrap();
        let RubyValue::Hash(h) = &v else {
            panic!("expected Hash")
        };
        assert!(matches!(
            crate::collections::hash_get(h, &s("a")),
            RubyValue::Int(1)
        ));
    }

    #[test]
    fn load_scalars() {
        assert!(matches!(
            f("load")(&RubyValue::Nil, &[s("42")], None).unwrap(),
            RubyValue::Int(42)
        ));
        assert!(matches!(
            f("load")(&RubyValue::Nil, &[s("hello")], None).unwrap(),
            RubyValue::Str(_)
        ));
    }

    #[test]
    fn dump_matches_ruby_block_style() {
        assert_eq!(dumped(&RubyValue::Nil), "---\n");
        assert_eq!(dumped(&RubyValue::Int(42)), "--- 42\n");
        let arr = RubyValue::Array(array_new(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            s("x"),
        ]));
        assert_eq!(dumped(&arr), "---\n- 1\n- 2\n- x\n");
        // A sequence nested under a key stays at the key's indent (Psych quirk).
        let h = RubyValue::Hash(hash_new(vec![
            (
                s("a"),
                RubyValue::Array(array_new(vec![RubyValue::Int(1), RubyValue::Int(2)])),
            ),
            (
                s("b"),
                RubyValue::Hash(hash_new(vec![(s("c"), RubyValue::Int(3))])),
            ),
        ]));
        assert_eq!(dumped(&h), "---\na:\n- 1\n- 2\nb:\n  c: 3\n");
    }
}
