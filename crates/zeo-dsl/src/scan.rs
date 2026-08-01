//! Reads the `ruby_class!`/`ruby_module!` declarations back out of the runtime
//! source, with the grammar in this crate's root. The walk lives beside the
//! grammar so every tool that needs to see what the runtime registers -- the
//! arity oracle, the annotator, the drift test -- reads it the one way.
//!
//! Also lifts the `ClassId`-const -> Ruby-name map out of `zeo-abi`'s
//! `BUILTINS` table. That is read as tokens rather than linked against, so no
//! const is ever evaluated: the rows spell `id: STRING_CLASS` as a bare ident,
//! which is all the join needs. The const is the only token the DSL header and
//! the ABI table share -- a header says `Stat` where the Ruby name is
//! `File::Stat`, and only `BUILTINS` knows that.
//!
//! `Decl::line` is best-effort: it is meaningful only when `proc-macro2`'s
//! `span-locations` feature is on (xtask enables it), and reads 0 otherwise.
//! Nothing depends on it for correctness -- it exists to make a failure message
//! point somewhere useful.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::parse::Parser;

use crate::ClassSpec;

/// Instance (`def foo`) vs singleton (`def self.foo`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    Instance,
    Singleton,
}

impl Kind {
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Instance => "i",
            Kind::Singleton => "s",
        }
    }
}

/// One Ruby name zeo declares, and where it was declared.
pub struct Decl {
    /// The class's `ClassId` const ident (`STRING_CLASS`), the join key.
    pub class_const: String,
    pub kind: Kind,
    pub name: String,
    /// What `Method#arity` will report: the same
    /// `name.arity.unwrap_or(def.derived_arity())` the proc-macro emits, so a
    /// tool reading this sees exactly what the runtime registers.
    pub arity: i64,
    /// What the def's parameter list alone implies, ignoring any override.
    pub derived: i64,
    /// Whether this name carries an explicit `arity N`.
    pub override_written: bool,
    /// Repo-relative, so the output is machine-independent.
    pub file: String,
    pub line: usize,
    /// The `#[cfg(...)]` gating this def, rendered back to source. A gated
    /// method may legitimately be absent from an oracle running on another
    /// platform, which is a different thing from zeo having invented it.
    pub cfg: Option<String>,
    /// Whether the def is `private`. A PRIVATE name CRuby lacks is an
    /// implementation seam, not surface -- it cannot be reached, listed, or
    /// `respond_to?`'d -- which is a different kind of divergence from a
    /// public method zeo invented.
    pub is_private: bool,
}

/// What `zeo-abi`'s `BUILTINS` says about one class.
pub struct AbiClass {
    pub ruby_name: String,
    pub is_module: bool,
    /// The `require` that exposes the class, if it is gated.
    pub feature: Option<String>,
    pub superclass: Option<String>,
    pub includes: Vec<String>,
}

/// Collect every declaration in the runtime. The walk covers all of
/// `crates/zeo-rt/src`, not just `builtins/` and `ext/`, because `Ractor` is
/// declared at the top level. A file that hosts the same-named `macro_rules!`
/// instead falls out on its own: those tokens do not parse as a `ClassSpec`.
pub fn scan_decls(root: &Path) -> Vec<Decl> {
    let mut out = Vec::new();
    walk_dir(&root.join("crates/zeo-rt/src"), root, &mut out);
    out.sort_by(|a, b| (&a.class_const, a.kind, &a.name).cmp(&(&b.class_const, b.kind, &b.name)));
    out
}

fn walk_dir(dir: &Path, root: &Path, out: &mut Vec<Decl>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk_dir(&path, root, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            scan_file(&path, root, out);
        }
    }
}

fn scan_file(path: &Path, root: &Path, out: &mut Vec<Decl>) {
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(file) = syn::parse_file(&source) else {
        return;
    };
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    scan_items(&file.items, &rel, out);
}

fn scan_items(items: &[syn::Item], rel: &str, out: &mut Vec<Decl>) {
    for item in items {
        match item {
            syn::Item::Macro(m) => {
                let Some(last) = m.mac.path.segments.last() else {
                    continue;
                };
                let tokens = m.mac.tokens.clone();
                let spec = match last.ident.to_string().as_str() {
                    "ruby_class" => ClassSpec::parse_class.parse2(tokens).ok(),
                    "ruby_module" => ClassSpec::parse_module.parse2(tokens).ok(),
                    _ => None,
                };
                if let Some(spec) = spec {
                    collect_spec(&spec, rel, out);
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    scan_items(inner, rel, out);
                }
            }
            _ => {}
        }
    }
}

/// Recurses into `spec.nested`, so a namespaced class (`Process::Status`) is
/// keyed by its OWN id const rather than its parent's.
fn collect_spec(spec: &ClassSpec, rel: &str, out: &mut Vec<Decl>) {
    let class_const = spec
        .id
        .segments
        .last()
        .expect("a ClassId path has at least one segment")
        .ident
        .to_string();

    for method in &spec.methods {
        let cfg = render_cfg(&method.attrs);
        let derived = method.derived_arity();
        // The receiver ident is the one token every def has that carries a
        // usable span today. Names gain their own spans when the annotator
        // needs byte ranges.
        let line = span_line(method.recv.span());
        // `module_function` defines the name as both an instance method and a
        // singleton method, exactly as CRuby does.
        let kinds: &[Kind] = if method.is_module_function {
            &[Kind::Instance, Kind::Singleton]
        } else if method.is_class_method {
            &[Kind::Singleton]
        } else {
            &[Kind::Instance]
        };
        for name in &method.names {
            for &kind in kinds {
                out.push(Decl {
                    class_const: class_const.clone(),
                    kind,
                    name: name.ruby.clone(),
                    arity: name.arity.unwrap_or(derived),
                    derived,
                    override_written: name.arity.is_some(),
                    file: rel.to_owned(),
                    line,
                    cfg: cfg.clone(),
                    is_private: method.visibility == crate::Visibility::Private,
                });
            }
        }
    }

    for nested in &spec.nested {
        collect_spec(nested, rel, out);
    }
}

#[cfg(feature = "spans")]
fn span_line(span: proc_macro2::Span) -> usize {
    span.start().line
}

#[cfg(not(feature = "spans"))]
fn span_line(_span: proc_macro2::Span) -> usize {
    0
}

fn render_cfg(attrs: &[syn::Attribute]) -> Option<String> {
    use quote::ToTokens;
    let rendered: Vec<String> = attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .map(|a| a.meta.to_token_stream().to_string().replace('\t', " "))
        .collect();
    (!rendered.is_empty()).then(|| rendered.join(" "))
}

/// Lift `zeo-abi`'s `BUILTINS` table symbolically: `ClassId` const ident ->
/// Ruby name, module-ness, `require` feature, superclass and includes.
pub fn scan_abi(root: &Path) -> BTreeMap<String, AbiClass> {
    let path = root.join("crates/zeo-abi/src/lib.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let file =
        syn::parse_file(&source).unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));

    let mut out = BTreeMap::new();
    for item in &file.items {
        let syn::Item::Const(c) = item else { continue };
        if c.ident != "BUILTINS" {
            continue;
        }
        let syn::Expr::Reference(r) = &*c.expr else {
            continue;
        };
        let syn::Expr::Array(arr) = &*r.expr else {
            continue;
        };
        for elem in &arr.elems {
            let syn::Expr::Struct(s) = elem else { continue };
            let mut id = None;
            let mut name = None;
            let mut is_module = false;
            let mut feature = None;
            let mut superclass = None;
            let mut includes = Vec::new();
            for field in &s.fields {
                let syn::Member::Named(key) = &field.member else {
                    continue;
                };
                match key.to_string().as_str() {
                    "id" => id = expr_ident(&field.expr),
                    "name" => name = expr_str(&field.expr),
                    "is_module" => is_module = expr_bool(&field.expr).unwrap_or(false),
                    "feature" => feature = expr_some_str(&field.expr),
                    "superclass" => superclass = expr_some_ident(&field.expr),
                    "includes" => includes = expr_ident_slice(&field.expr),
                    _ => {}
                }
            }
            if let (Some(id), Some(name)) = (id, name) {
                out.insert(
                    id,
                    AbiClass {
                        ruby_name: name,
                        is_module,
                        feature,
                        superclass,
                        includes,
                    },
                );
            }
        }
    }
    out
}

fn expr_ident(e: &syn::Expr) -> Option<String> {
    match e {
        syn::Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

fn expr_str(e: &syn::Expr) -> Option<String> {
    match e {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => Some(s.value()),
        _ => None,
    }
}

fn expr_bool(e: &syn::Expr) -> Option<bool> {
    match e {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(b),
            ..
        }) => Some(b.value()),
        _ => None,
    }
}

/// `Some("base64")` -> `Some("base64")`; `None` -> `None`.
fn expr_some_str(e: &syn::Expr) -> Option<String> {
    let syn::Expr::Call(call) = e else {
        return None;
    };
    if expr_ident(&call.func).as_deref() != Some("Some") {
        return None;
    }
    call.args.first().and_then(expr_str)
}

fn expr_some_ident(e: &syn::Expr) -> Option<String> {
    let syn::Expr::Call(call) = e else {
        return None;
    };
    if expr_ident(&call.func).as_deref() != Some("Some") {
        return None;
    }
    call.args.first().and_then(expr_ident)
}

fn expr_ident_slice(e: &syn::Expr) -> Vec<String> {
    let syn::Expr::Reference(r) = e else {
        return Vec::new();
    };
    let syn::Expr::Array(arr) = &*r.expr else {
        return Vec::new();
    };
    arr.elems.iter().filter_map(expr_ident).collect()
}
