//! One-off: turn the e2e Rust tests that only run a program and compare its
//! output into corpus programs.
//!
//! A test whose body is one `run_ruby*` call on literal sources becomes
//! `test/_e2e/<module>/<fn>.rb` (a multi-file project becomes a stub plus a
//! `<fn>/` directory); a `compile_project` negative test becomes an
//! `errors/` program. Everything else is listed for the move to `api/`.
//! The discarded `assert_eq!` stdout goes into the report so a bless that
//! records something else is reviewed. Deleted once the migration lands.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMacro, ExprPath, Lit};

use crate::{Error, root_join};

const USAGE: &str = "usage: cargo xtask migrate-e2e";

const RUN_HELPERS: &[&str] = &[
    "run_ruby",
    "run_ruby_boxed",
    "run_ruby_configured",
    "run_ruby_project",
    "run_ruby_project_boxed",
    "run_ruby_project_embedded",
    "compile_project",
    "compile_project_strict",
];

/// Anything a converted program cannot express.
const KEEP_MARKERS: &[&str] = &[
    "run_ruby_packages",
    "compile_packages",
    "write_project",
    "extension_dir",
    "gem_store",
    "have",
    "Command",
    "zeo_cli",
    "runtime_archive",
];

pub fn run(args: &[String]) -> Result<(), Error> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let dir = root_join("crates/zeo/tests/e2e");
    let out_root = root_join("test/_e2e");
    let mut report = String::from("module\tfn\tkind\tdetail\n");
    let mut converted = 0;
    let mut kept = 0;
    let mut api_modules: Vec<String> = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| Error::new(format!("{}: {e}", dir.display())))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "rs")
                && p.file_name().is_some_and(|n| n != "main.rs")
        })
        .collect();
    entries.sort();
    for path in entries {
        let module = path.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
        let file =
            syn::parse_file(&text).map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
        let lines: Vec<&str> = text.lines().collect();
        let mut removed: Vec<(usize, usize)> = Vec::new();
        for item in &file.items {
            let syn::Item::Fn(f) = item else { continue };
            if !f.attrs.iter().any(|a| a.path().is_ident("test")) {
                continue;
            }
            let name = f.sig.ident.to_string();
            match convert(f, &lines, &module) {
                Ok(Converted {
                    kind,
                    program,
                    files,
                    expected,
                    notes,
                }) => {
                    let first = f
                        .attrs
                        .first()
                        .map(|a| a.pound_token.span.start().line)
                        .unwrap_or(f.sig.fn_token.span.start().line);
                    let last = f.block.brace_token.span.close().end().line;
                    removed.push((first, last));
                    let module_dir = out_root.join(&module);
                    std::fs::create_dir_all(&module_dir).map_err(|e| Error::new(e.to_string()))?;
                    std::fs::write(module_dir.join(format!("{name}.rb")), &program)
                        .map_err(|e| Error::new(e.to_string()))?;
                    for (rel, src) in &files {
                        let p = module_dir.join(&name).join(rel);
                        std::fs::create_dir_all(p.parent().unwrap())
                            .map_err(|e| Error::new(e.to_string()))?;
                        std::fs::write(&p, src).map_err(|e| Error::new(e.to_string()))?;
                    }
                    let mut detail = String::new();
                    if let Some(e) = expected {
                        write!(detail, "stdout={e:?} ").unwrap();
                    }
                    for n in notes {
                        write!(detail, "{n} ").unwrap();
                    }
                    writeln!(report, "{module}\t{name}\t{kind}\t{}", detail.trim_end()).unwrap();
                    converted += 1;
                }
                Err(why) => {
                    writeln!(report, "{module}\t{name}\tkeep\t{why}").unwrap();
                    kept += 1;
                }
            }
        }
        // What survives goes to api/, minus the converted functions (their
        // doc comments went with them) and any blank run they leave.
        let survivors = file
            .items
            .iter()
            .filter(|i| matches!(i, syn::Item::Fn(f) if f.attrs.iter().any(|a| a.path().is_ident("test"))))
            .count()
            - removed.len();
        if survivors > 0 {
            let mut kept_lines: Vec<&str> = Vec::new();
            for (n, line) in lines.iter().enumerate() {
                let ln = n + 1;
                if removed.iter().any(|(a, b)| ln >= *a && ln <= *b) {
                    continue;
                }
                if line.trim().is_empty() && kept_lines.last().is_some_and(|l| l.trim().is_empty())
                {
                    continue;
                }
                kept_lines.push(line);
            }
            let api = root_join("crates/zeo/tests/api");
            std::fs::create_dir_all(&api).map_err(|e| Error::new(e.to_string()))?;
            let mut out = kept_lines.join("\n");
            out.push('\n');
            std::fs::write(api.join(format!("{module}.rs")), out)
                .map_err(|e| Error::new(e.to_string()))?;
            api_modules.push(module.clone());
        }
    }
    std::fs::create_dir_all(&out_root).map_err(|e| Error::new(e.to_string()))?;
    std::fs::write(out_root.join("report.tsv"), report).map_err(|e| Error::new(e.to_string()))?;
    eprintln!("api modules: {}", api_modules.join(" "));
    eprintln!("migrate-e2e: {converted} converted, {kept} kept (test/_e2e/report.tsv)");
    Ok(())
}

struct Converted {
    kind: &'static str,
    program: String,
    files: Vec<(String, String)>,
    expected: Option<String>,
    notes: Vec<String>,
}

/// Every call and path in a function body, by name.
#[derive(Default)]
struct Calls<'a> {
    calls: Vec<&'a ExprCall>,
    idents: Vec<String>,
    macros: Vec<&'a ExprMacro>,
    stmt_macros: Vec<&'a syn::StmtMacro>,
}

impl<'a> Visit<'a> for Calls<'a> {
    fn visit_expr_call(&mut self, c: &'a ExprCall) {
        self.calls.push(c);
        syn::visit::visit_expr_call(self, c);
    }
    fn visit_expr_path(&mut self, p: &'a ExprPath) {
        for seg in &p.path.segments {
            self.idents.push(seg.ident.to_string());
        }
        syn::visit::visit_expr_path(self, p);
    }
    fn visit_expr_macro(&mut self, m: &'a ExprMacro) {
        self.macros.push(m);
        syn::visit::visit_expr_macro(self, m);
    }
    fn visit_stmt_macro(&mut self, m: &'a syn::StmtMacro) {
        self.stmt_macros.push(m);
        syn::visit::visit_stmt_macro(self, m);
    }
    fn visit_expr_method_call(&mut self, c: &'a syn::ExprMethodCall) {
        self.idents.push(c.method.to_string());
        syn::visit::visit_expr_method_call(self, c);
    }
}

fn call_name(c: &ExprCall) -> Option<String> {
    match &*c.func {
        Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

fn str_lit(e: &Expr) -> Option<String> {
    match e {
        Expr::Lit(l) => match &l.lit {
            Lit::Str(s) => Some(s.value()),
            _ => None,
        },
        Expr::Reference(r) => str_lit(&r.expr),
        Expr::Paren(p) => str_lit(&p.expr),
        _ => None,
    }
}

/// `&[("a", "b"), ...]`
fn pairs_lit(e: &Expr) -> Option<Vec<(String, String)>> {
    let arr = match e {
        Expr::Reference(r) => match &*r.expr {
            Expr::Array(a) => a,
            _ => return None,
        },
        Expr::Array(a) => a,
        _ => return None,
    };
    let mut out = Vec::new();
    for el in &arr.elems {
        let Expr::Tuple(t) = el else { return None };
        if t.elems.len() != 2 {
            return None;
        }
        out.push((str_lit(&t.elems[0])?, str_lit(&t.elems[1])?));
    }
    Some(out)
}

/// `&["a", "b"]`
fn strs_lit(e: &Expr) -> Option<Vec<String>> {
    let arr = match e {
        Expr::Reference(r) => match &*r.expr {
            Expr::Array(a) => a,
            _ => return None,
        },
        Expr::Array(a) => a,
        _ => return None,
    };
    arr.elems.iter().map(str_lit).collect()
}

fn dedent(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out: Vec<&str> = lines
        .iter()
        .map(|l| {
            if l.len() >= indent {
                &l[indent..]
            } else {
                l.trim_start()
            }
        })
        .collect();
    while out.first().is_some_and(|l| l.trim().is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

fn convert(f: &syn::ItemFn, lines: &[&str], module: &str) -> Result<Converted, String> {
    let mut calls = Calls::default();
    calls.visit_block(&f.block);
    for marker in KEEP_MARKERS {
        if calls.idents.iter().any(|i| i == marker) {
            return Err(format!("uses {marker}"));
        }
    }
    if calls.idents.iter().any(|i| i == "zeo") {
        return Err("uses the zeo library".into());
    }
    // `eval.rs` wraps run_ruby in `run(src)` and `agree(src, expected)`.
    let local: &[&str] = if module == "eval" {
        &["run", "agree"]
    } else {
        &[]
    };
    let runs: Vec<&ExprCall> = calls
        .calls
        .iter()
        .copied()
        .filter(|c| {
            call_name(c)
                .is_some_and(|n| RUN_HELPERS.contains(&n.as_str()) || local.contains(&n.as_str()))
        })
        .collect();
    if runs.len() != 1 {
        return Err(format!("{} run calls", runs.len()));
    }
    let run = runs[0];
    let helper = call_name(run).unwrap();
    let args: Vec<&Expr> = run.args.iter().collect();

    let mut directives: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();
    let mut kind = "convert";
    let mut env: Vec<(String, String)> = Vec::new();
    let mut argv: Vec<String> = Vec::new();
    let mut zeo_flags: Vec<String> = Vec::new();
    let mut load_roots: Vec<String> = Vec::new();

    let mut expected = None;
    let program_body: String = match helper.as_str() {
        "run_ruby" | "run" => dedent(&str_lit(args[0]).ok_or("program is not a literal")?),
        "agree" => {
            expected = str_lit(args[1]);
            dedent(&str_lit(args[0]).ok_or("program is not a literal")?)
        }
        "run_ruby_boxed" => {
            env.push(("RUBY_BOX".into(), "1".into()));
            dedent(&str_lit(args[0]).ok_or("program is not a literal")?)
        }
        "run_ruby_configured" => {
            let src = str_lit(args[0]).ok_or("program is not a literal")?;
            env.extend(pairs_lit(args[1]).ok_or("env is not literal")?);
            argv.extend(strs_lit(args[2]).ok_or("args are not literal")?);
            dedent(&src)
        }
        "run_ruby_project"
        | "run_ruby_project_boxed"
        | "run_ruby_project_embedded"
        | "compile_project"
        | "compile_project_strict" => {
            let project = pairs_lit(args[0]).ok_or("files are not literal")?;
            let entry = str_lit(args[1]).ok_or("entry is not a literal")?;
            let roots = strs_lit(args[2]).ok_or("roots are not literal")?;
            if helper == "run_ruby_project_boxed" {
                env.push(("RUBY_BOX".into(), "1".into()));
            }
            if helper == "run_ruby_project_embedded" {
                let embed = strs_lit(args[3]).ok_or("embed roots not literal")?;
                for e in &embed {
                    zeo_flags.push(format!("--embed-sources {}/{e}", f.sig.ident));
                    load_roots.push(e.clone());
                }
                env.extend(pairs_lit(args[4]).ok_or("env not literal")?);
            }
            if helper.starts_with("compile_project") {
                kind = "errors";
                if helper == "compile_project_strict" {
                    zeo_flags.push("--strict-static-require".into());
                }
            }
            for (rel, src) in project {
                files.push((rel, dedent(&src)));
            }
            load_roots.extend(roots);
            let name = f.sig.ident.to_string();
            let mut stub = String::new();
            for r in &load_roots {
                let r = r.trim_matches('/');
                let dir = if r.is_empty() || r == "." {
                    name.clone()
                } else {
                    format!("{name}/{r}")
                };
                writeln!(
                    stub,
                    "$LOAD_PATH.unshift(File.expand_path({dir:?}, __dir__))"
                )
                .unwrap();
            }
            let entry = entry.trim_end_matches(".rb");
            writeln!(stub, "require_relative {:?}", format!("{name}/{entry}")).unwrap();
            stub
        }
        other => return Err(format!("unknown helper {other}")),
    };

    // What the test asserted, for the review after bless.
    let mut expects_failure = false;
    let all_macros: Vec<(&syn::Macro,)> = calls
        .macros
        .iter()
        .map(|m| (&m.mac,))
        .chain(calls.stmt_macros.iter().map(|m| (&m.mac,)))
        .collect();
    for (mac,) in all_macros {
        let Some(mname) = mac.path.get_ident().map(|i| i.to_string()) else {
            continue;
        };
        let parsed = mac
            .parse_body_with(syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated);
        let Ok(margs) = parsed else { continue };
        let texts: Vec<String> = margs
            .iter()
            .map(|e| quote::ToTokens::to_token_stream(e).to_string())
            .collect();
        match mname.as_str() {
            "assert_eq" if texts.first().is_some_and(|t| t.contains("stdout")) => {
                if let Some(lit) = margs.iter().nth(1).and_then(str_lit) {
                    expected = Some(lit);
                } else {
                    notes.push("stdout-compared-to-non-literal".into());
                }
            }
            "assert" => {
                let t = texts.first().cloned().unwrap_or_default();
                if t.contains("! result . status . success") || t.contains("!result.status.success")
                {
                    expects_failure = true;
                } else if t.contains("contains") {
                    notes.push(format!("contains:{}", t.replace(' ', "")));
                } else if t.contains("code ()") {
                    notes.push(format!("code:{}", t.replace(' ', "")));
                }
            }
            _ => {}
        }
    }
    if expects_failure {
        notes.push("expects-nonzero-exit".into());
    }

    // The comments a person wrote: the doc comment above the fn, and the
    // line comments inside it.
    let mut header: Vec<String> = Vec::new();
    for attr in &f.attrs {
        if let syn::Meta::NameValue(nv) = &attr.meta
            && nv.path.is_ident("doc")
            && let Expr::Lit(l) = &nv.value
            && let Lit::Str(s) = &l.lit
        {
            header.push(s.value().trim_start().to_string());
        }
    }
    let start = f.block.brace_token.span.open().start().line;
    let end = f.block.brace_token.span.close().end().line;
    let mut body_comments: Vec<String> = Vec::new();
    for line in lines.iter().take(end).skip(start.saturating_sub(1)) {
        let t = line.trim_start();
        if let Some(c) = t.strip_prefix("//") {
            if !c.starts_with('/') && !c.starts_with('!') {
                body_comments.push(c.trim_start().to_string());
            }
        }
    }
    if !header.is_empty() && !body_comments.is_empty() {
        header.push(String::new());
    }
    header.extend(body_comments);

    let mut program = String::new();
    for h in &header {
        if h.is_empty() {
            program.push_str("#\n");
        } else {
            writeln!(program, "# {h}").unwrap();
        }
    }
    if program_body.contains("Ruby::Box") || files.iter().any(|(_, src)| src.contains("Ruby::Box"))
    {
        directives.push("#@ ruby: -W:no-experimental".into());
    }
    if !env.is_empty() {
        let e: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        directives.push(format!("#@ env: {}", e.join(" ")));
    }
    if !argv.is_empty() {
        directives.push(format!("#@ args: {}", argv.join(" ")));
    }
    if !zeo_flags.is_empty() {
        directives.push(format!("#@ zeo: {}", zeo_flags.join(" ")));
    }
    for d in &directives {
        program.push_str(d);
        program.push('\n');
    }
    if !header.is_empty() || !directives.is_empty() {
        program.push('\n');
    }
    program.push_str(&program_body);
    Ok(Converted {
        kind,
        program,
        files,
        expected,
        notes,
    })
}

#[allow(dead_code)]
fn _unused(_: &Path) {}
