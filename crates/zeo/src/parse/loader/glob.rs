//! Glob-require expansion: `Dir.glob`/`Dir[]` calls whose block just
//! requires each match are compile-time facts, expanded here so the matching
//! files splice as ordinary requires.

use std::path::Path;

/// The `Dir.glob`/`Dir[]` calls whose block just requires each match --
/// `Dir.glob("#{__dir__}/smtp/auth_*.rb") { |r| require_relative r }`, how
/// net/smtp loads its authenticators and how rubygems loads its plugins.
/// Collected exactly like `collect_autoloads`, and expanded the same way: the
/// pattern is a compile-time fact, so the matching files are spliced as
/// ordinary requires.
pub(super) fn collect_glob_requires<'a>(
    node: &ruby_prism::Node<'a>,
    out: &mut Vec<ruby_prism::CallNode<'a>>,
) {
    if let Some(stmts) = node.as_statements_node() {
        for n in stmts.body().iter() {
            collect_glob_requires(&n, out);
        }
    } else if let Some(m) = node.as_module_node() {
        if let Some(body) = m.body() {
            collect_glob_requires(&body, out);
        }
    } else if let Some(c) = node.as_class_node() {
        if let Some(body) = c.body() {
            collect_glob_requires(&body, out);
        }
    } else if let Some(call) = node.as_call_node() {
        out.push(call);
    }
}

/// The files a glob-and-require pattern matches, sorted, as absolute paths.
///
/// Deliberately narrow: only a `*` in the FINAL segment is expanded (no `**`,
/// no character classes, no brace expansion), because that is the shape the
/// require idiom uses -- `auth_*.rb`, `*.rb` -- and a partial implementation of
/// the rest would be worse than none. A pattern with a wildcard anywhere else
/// matches nothing here and lowers as ordinary runtime code.
pub(super) fn expand_glob(pattern: &str) -> Vec<String> {
    let (dir, leaf) = match pattern.rsplit_once('/') {
        Some((d, l)) => (Path::new(d), l),
        None => return Vec::new(),
    };
    if dir.to_string_lossy().contains('*') || !leaf.contains('*') {
        return Vec::new();
    }
    let (prefix, suffix) = leaf.split_once('*').expect("checked above");
    if suffix.contains('*') {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let matches = name.len() >= prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix);
            matches.then(|| dir.join(&name).to_string_lossy().into_owned())
        })
        .collect();
    // Sorted so the splice order is deterministic across filesystems -- the
    // requires land in the program in a fixed sequence, as `Dir.glob`'s own
    // (sorted by default since ruby 3.0) does.
    out.sort();
    out
}

/// One recognized glob-and-require: the shell pattern (with `__dir__` already
/// resolved against `dir`) and the require flavor its block uses.
pub(super) struct GlobRequire {
    pub(super) pattern: String,
    pub(super) flavor: String,
}

/// Whether `call` is a glob-and-require. Accepts the direct block form and the
/// `.each`/`.sort.each` chains, which are the spellings in the wild; the block
/// must do nothing but require its own parameter, so nothing else is silently
/// dropped. `None` for every other `Dir.glob`, which lowers as ordinary code.
pub(super) fn glob_require_call(
    call: &ruby_prism::CallNode<'_>,
    dir: Option<&Path>,
) -> Option<GlobRequire> {
    // The block rides on the OUTERMOST call; the pattern is under whatever
    // `.each`/`.sort` chain leads back to `Dir.glob`.
    let block = call.block()?.as_block_node()?;
    let pattern = dir_glob_pattern(call, dir)?;

    // The block must be exactly `require_relative <its own param>`.
    let params = block.parameters()?.as_block_parameters_node()?;
    let param = params.parameters()?.requireds().iter().next()?;
    let param = param.as_required_parameter_node()?;
    let body = block.body()?;
    let body = body.as_statements_node()?;
    let stmts: Vec<_> = body.body().iter().collect();
    let [only] = stmts.as_slice() else {
        return None;
    };
    let req = only.as_call_node()?;
    let flavor = String::from_utf8_lossy(req.name().as_slice()).into_owned();
    if req.receiver().is_some() || !matches!(flavor.as_str(), "require" | "require_relative") {
        return None;
    }
    let req_args: Vec<_> = req
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let [req_arg] = req_args.as_slice() else {
        return None;
    };
    (req_arg.as_local_variable_read_node()?.name().as_slice() == param.name().as_slice())
        .then_some(GlobRequire { pattern, flavor })
}

/// The shell pattern behind `Dir.glob(...)` / `Dir[...]`, peeling any
/// `.each`/`.sort`/`.to_a` chain in front of it.
pub(super) fn dir_glob_pattern(
    call: &ruby_prism::CallNode<'_>,
    dir: Option<&Path>,
) -> Option<String> {
    match call.name().as_slice() {
        b"each" | b"sort" | b"to_a" => {
            let recv = call.receiver()?;
            dir_glob_pattern(&recv.as_call_node()?, dir)
        }
        b"glob" | b"[]" => {
            if call.receiver()?.as_constant_read_node()?.name().as_slice() != b"Dir" {
                return None;
            }
            let args: Vec<_> = call
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            let [pattern] = args.as_slice() else {
                return None;
            };
            glob_pattern_text(pattern, dir)
        }
        _ => None,
    }
}

/// A glob pattern's text, if every piece is known at compile time: a plain
/// string literal, or an interpolation whose only computed part is `__dir__`
/// (`"#{__dir__}/smtp/auth_*.rb"`), which resolves to the requiring file's own
/// directory.
pub(super) fn glob_pattern_text(node: &ruby_prism::Node<'_>, dir: Option<&Path>) -> Option<String> {
    if let Some(s) = node.as_string_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    let interp = node.as_interpolated_string_node()?;
    let mut out = String::new();
    for part in interp.parts().iter() {
        if let Some(s) = part.as_string_node() {
            out.push_str(&String::from_utf8_lossy(s.unescaped()));
            continue;
        }
        let embedded = part.as_embedded_statements_node()?;
        let stmts: Vec<_> = embedded.statements()?.body().iter().collect();
        let [only] = stmts.as_slice() else {
            return None;
        };
        let call = only.as_call_node()?;
        if call.receiver().is_some() || call.name().as_slice() != b"__dir__" {
            return None;
        }
        out.push_str(dir?.to_str()?);
    }
    Some(out)
}
