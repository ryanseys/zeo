//! Projects the compiler's view of the builtin METHOD/CONSTANT surface, and
//! renders the target-derived `rbconfig` shim.
//!
//! The compiler folds `respond_to?`/`method_defined?`/const lookups against a
//! class's known members. For USER classes it reads the compiled program; for
//! BUILTINS that data lives in `zeo-rt` (never linked into the compiler). This
//! build script bridges the gap WITHOUT a cargo dependency edge (which would be
//! a cycle). Two sources, tried in order:
//!
//! - **Dev tree** (sibling `../zeo-rt/src` exists): read
//!   `zeo-rt/src/{builtins,ext}/*.rs` as SOURCE, find every
//!   `ruby_class!`/`ruby_module!` invocation, and parse each with the SHARED
//!   `zeo-dsl` grammar -- the exact parser the proc-macro uses, so the
//!   compiler's folding view can never drift from what the runtime registers.
//! - **Packaged crate** (no siblings -- a published `.crate` building out of
//!   the registry): use `src/class_surface.pregen.rs`, the projection
//!   `cargo xtask stage-publish` generated from the exact zeo-rt this zeo
//!   version pins (`=X.Y.Z`), staged into the crate at publish time.
//!
//! NEITHER source resolving is a hard error. It used to degrade silently to
//! an empty surface, which quietly broke every fold -- worse than any build
//! failure.
//!
//! It emits `$OUT_DIR/class_surface.rs`: a `CLASS_SURFACE` table written
//! SYMBOLICALLY (`id: zeo_abi::COMPARABLE_CLASS`), so the build script never
//! evaluates a `ClassId` const -- rustc resolves the symbol when it compiles
//! the include (see `src/builtin_surface.rs`). Only classes already migrated to
//! the macro appear; the rest stay served by `zeo_abi::BUILTINS` shape + the
//! compiler's existing (surface-blind) behavior until they migrate too.

use std::fmt::Write as _;
use std::path::Path;

use syn::parse::Parser;
use zeo_dsl::ClassSpec;

fn main() {
    // The export flags exist for the C-API crate: without it there is no
    // `rb_*` for an extension to resolve.
    if std::env::var_os("CARGO_FEATURE_CAPI").is_some() {
        export_cext_surface();
    }
    size_the_main_thread_stack();

    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let manifest_dir = Path::new(&manifest_dir);
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR set by cargo");
    let out_dir = Path::new(&out_dir);

    // The embedded gems archive (`cargo xtask stage-publish` creates it; the
    // published .crate ships it). Its presence arms the Registry home tier.
    println!("cargo:rustc-check-cfg=cfg(zeo_embedded_gems)");
    println!("cargo:rerun-if-changed=gems.pregen.tar.gz");
    if manifest_dir.join("gems.pregen.tar.gz").is_file() {
        println!("cargo:rustc-cfg=zeo_embedded_gems");
    }

    let rt_src = manifest_dir.join("../zeo-rt/src");
    let pregen = manifest_dir.join("src/class_surface.pregen.rs");
    let dev_tree = rt_src.join("builtins").is_dir();

    let rt_ext = manifest_dir.join("../zeo-rt/ext");
    let code = if dev_tree {
        generate_class_surface(&rt_src, &rt_ext)
    } else if pregen.is_file() {
        println!("cargo:rerun-if-changed=src/class_surface.pregen.rs");
        std::fs::read_to_string(&pregen).expect("reading class_surface.pregen.rs")
    } else {
        panic!(
            "zeo's build script found NEITHER the zeo-rt sibling sources (a dev \
             tree) nor src/class_surface.pregen.rs (a published crate, staged by \
             `cargo xtask stage-publish`). The builtin class surface cannot be \
             projected, and building without it would silently disable the \
             compiler's respond_to?/method_defined?/constant folding. If you are \
             building from a source checkout, the full workspace is required."
        );
    };
    write_if_changed(&out_dir.join("class_surface.rs"), &code);

    render_rbconfig(manifest_dir, out_dir);
}

/// Write only when the content actually differs. rustc's dep-info tracks the
/// `include!`d `$OUT_DIR` files by mtime, so an identical rewrite would still
/// recompile this whole crate every time the script re-runs -- and the script
/// re-runs on every zeo-rt source edit, most of which don't touch a header.
fn write_if_changed(path: &Path, content: &str) {
    if std::fs::read_to_string(path).is_ok_and(|old| old == content) {
        return;
    }
    std::fs::write(path, content).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

/// Project `CLASS_SURFACE` from the sibling zeo-rt sources: the core classes
/// (`src/builtins/`) and the require-gated extensions (`ext/`, one gem-shaped
/// directory each, whose Rust sits at `ext/<name>/ext/<name>/src/`). Ext
/// surfaces are projected unconditionally of their cargo feature: the surface
/// is folding-only (a miss falls back to runtime dispatch), and an un-required
/// ext constant is unreachable regardless.
///
/// The built-in exception tree is deliberately ABSENT: `builtins/exception.rs`
/// has no `ruby_class!` invocation for this scanner to find -- its rows are a
/// runtime-installed table (`EXC_ROWS`), one shared implementation for all
/// ~171 classes -- so the compiler stays surface-blind there and every
/// exception-method call resolves through runtime dispatch.
fn generate_class_surface(rt_src: &Path, rt_ext: &Path) -> String {
    let mut surfaces: Vec<Surface> = Vec::new();
    collect_from_dir(&rt_src.join("builtins"), &mut surfaces);
    collect_from_dir(rt_ext, &mut surfaces);
    assert!(
        !surfaces.is_empty(),
        "projected an EMPTY class surface from {} -- the zeo-rt sources are \
         present but no ruby_class!/ruby_module! invocation parsed, which \
         would silently disable the compiler's builtin folding",
        rt_src.display()
    );
    // Deterministic output regardless of readdir order.
    surfaces.sort_by(|a, b| a.id_const.cmp(&b.id_const));

    // Every table symbol in the runtime, from the WHOLE tree rather than the
    // two directories the folding surface reads. The lists differ: `Ractor`
    // lives at `src/ractor.rs` and `FFI::Type` outside the ext that owns it,
    // so a surface-derived list was five short -- and a class whose table an
    // emitted program never names loses every method it has.
    // `class_tables_are_complete` gates the two against `libzeo.a` itself.
    let mut every = collect_table_ids(rt_src);
    every.extend(collect_table_ids(rt_ext));
    every.sort();
    every.dedup();

    let mut code =
        String::from("// @generated by build.rs from ruby_class!/ruby_module! headers.\n");
    code.push_str("pub const CLASS_TABLE_SYMBOLS: &[(ClassId, &str)] = &[\n");
    for id in &every {
        writeln!(code, "    (zeo_abi::{id}, \"zeo_ctable_{id}\"),")
            .expect("writing to a String never fails");
    }
    code.push_str("];\n");
    code.push_str("pub const CLASS_SURFACE: &[ClassSurface] = &[\n");
    for s in &surfaces {
        let superclass = match &s.superclass {
            Some(c) => format!("Some(zeo_abi::{c})"),
            None => "None".to_string(),
        };
        let includes = s
            .includes
            .iter()
            .map(|c| format!("zeo_abi::{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            code,
            "    ClassSurface {{ id: zeo_abi::{}, table_symbol: {:?}, header_name: {:?}, \
             is_module: {}, superclass: {}, includes: &[{}], instance_methods: &[{}], \
             class_methods: &[{}], constants: &[{}] }},",
            s.id_const,
            format!("zeo_ctable_{}", s.id_const),
            s.header_name,
            s.is_module,
            superclass,
            includes,
            str_list(&s.instance_methods),
            str_list(&s.class_methods),
            str_list(&s.constants),
        )
        .expect("writing to a String never fails");
    }
    code.push_str("];\n");
    code
}

/// Render `shims/rbconfig.rb.in` -> `$OUT_DIR/rbconfig.rb`, substituting the
/// build TARGET's platform facts (mirroring how CRuby's `configure` bakes
/// them). The naming logic deliberately duplicates `crates/zeo-rt/build.rs`
/// (`ruby_platform` and friends). A path `include!` across crates would break
/// in a packaged crate, and a cargo edge is heavier than the duplication.
/// Keep the two in sync.
fn render_rbconfig(manifest_dir: &Path, out_dir: &Path) {
    let template_path = manifest_dir.join("src/parse/shims/rbconfig.rb.in");
    println!("cargo:rerun-if-changed=src/parse/shims/rbconfig.rb.in");
    let template = std::fs::read_to_string(&template_path).expect("reading rbconfig.rb.in");

    let arch = ruby_arch();
    let os = ruby_os();
    // The same string the compiled program's `RUBY_PLATFORM` will hold (the
    // runtime derives its own copy from ITS build target -- see
    // `zeo-rt/build.rs`), so `guard_fold` can decide `if RUBY_PLATFORM ==
    // 'java'` and friends the way the program itself would answer.
    println!("cargo:rustc-env=ZEO_RUBY_PLATFORM={arch}-{os}");
    // The last field is `DLDFLAGS`: a C extension is a bundle or shared
    // object that resolves the runtime's `rb_*` against the HOST binary at
    // load time, so its own link must permit them to be undefined. The two
    // linkers spell that differently, and getting it wrong reads as a missing
    // implementation ("Undefined symbols ... _rb_define_method") when it is
    // only a link-line flag. The oracle's own `DLDFLAGS` carries the same.
    let (vendor, host_os, dlext, soext, ldshared, undefined) = match target_os().as_str() {
        "macos" | "ios" | "tvos" | "watchos" => (
            "apple",
            format!("darwin{}", darwin_major()),
            "bundle",
            "dylib",
            "clang -dynamic -bundle",
            "-Wl,-undefined,dynamic_lookup",
        ),
        "linux" => (
            "pc",
            "linux-gnu".to_string(),
            "so",
            "so",
            "cc -shared",
            "-Wl,--allow-shlib-undefined",
        ),
        other => (
            "unknown",
            other.to_string(),
            "so",
            "so",
            "cc -shared",
            "-Wl,--allow-shlib-undefined",
        ),
    };
    // The same `RbConfig::CONFIG` entries the shim above renders, exported so
    // a compile-time guard can read them without parsing the shim -- gems
    // spell the platform question as `RbConfig::CONFIG['host_os'] =~ /linux/`
    // at least as often as they spell it `RUBY_PLATFORM`.
    println!("cargo:rustc-env=ZEO_HOST_OS={host_os}");
    println!("cargo:rustc-env=ZEO_HOST_CPU={arch}");
    println!("cargo:rustc-env=ZEO_DLEXT={dlext}");
    println!("cargo:rustc-env=ZEO_SOEXT={soext}");
    let rendered = template
        .replace("@RUBY_PLATFORM@", &format!("{arch}-{os}"))
        .replace("@HOST_TRIPLE@", &format!("{arch}-{vendor}-{host_os}"))
        .replace("@HOST_CPU@", &arch)
        .replace("@HOST_VENDOR@", vendor)
        .replace("@HOST_OS@", &host_os)
        .replace("@DLEXT@", dlext)
        .replace("@SOEXT@", soext)
        .replace("@OS_VERSION@", &darwin_major())
        .replace("@LDSHARED@", ldshared)
        .replace("@UNDEFINED_FLAG@", undefined);
    assert!(
        !rendered.contains('@') || !rendered.contains("@RUBY"),
        "rbconfig.rb.in has an unsubstituted placeholder"
    );
    write_if_changed(&out_dir.join("rbconfig.rb"), &rendered);
}

fn target_os() -> String {
    std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default()
}

/// Ruby names a few CPUs differently from Rust's `target_arch`, and one of
/// them per-OS: Apple rubies say `arm64` (`arm64-darwin25`) while Linux
/// rubies keep `aarch64` (`aarch64-linux`). Everything else passes through
/// unchanged.
fn ruby_arch() -> String {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    match (arch.as_str(), target_os().as_str()) {
        ("aarch64", "macos" | "ios" | "tvos" | "watchos") => "arm64".to_string(),
        ("x86", _) => "i686".to_string(),
        _ => arch,
    }
}

/// Ruby's OS token: `darwin<major>` on Apple targets, `linux`/`linux-musl`
/// on Linux, the raw `target_os` elsewhere.
fn ruby_os() -> String {
    match target_os().as_str() {
        "macos" | "ios" | "tvos" | "watchos" => format!("darwin{}", darwin_major()),
        "linux" => match std::env::var("CARGO_CFG_TARGET_ENV")
            .unwrap_or_default()
            .as_str()
        {
            "musl" => "linux-musl".to_string(),
            _ => "linux".to_string(),
        },
        other => other.to_string(),
    }
}

/// The Darwin kernel major (`uname -r` -> `25.5.0` -> `25`) when building ON
/// a mac FOR a mac; a pinned contemporary default when cross-compiling (the
/// build host's kernel is meaningless for the target then). Only reached for
/// Apple targets.
fn darwin_major() -> String {
    if std::env::consts::OS == "macos"
        && let Some(major) = std::process::Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|r| r.trim().split('.').next().map(str::to_string))
    {
        return major;
    }
    // ruby 4.0.6 era: Darwin 25 (macOS 26). Cosmetic (RUBY_PLATFORM suffix).
    "25".to_string()
}

/// One builtin class's projected surface.
struct Surface {
    /// The `ClassId` const's name (e.g. `COMPARABLE_CLASS`), emitted as
    /// `zeo_abi::<name>` for rustc to resolve.
    id_const: String,
    /// The header identifier (`String`, `Comparable`). A Rust `Ident`, so a
    /// namespaced Ruby name (`FFI::Type::Builtin`) can only be approximated --
    /// which is why the shape test compares it loosely.
    header_name: String,
    is_module: bool,
    /// `< SUPER`'s const name; `None` for a module and for `BasicObject`.
    superclass: Option<String>,
    includes: Vec<String>,
    instance_methods: Vec<String>,
    class_methods: Vec<String>,
    constants: Vec<String>,
}

/// Does every `#[cfg]` on this item hold for the target `zeo` is built for?
///
/// The projection used to push every row, so the surface was the UNION of
/// every platform -- and `respond_to?` folded TRUE for a method the target
/// does not compile (`Etc::Passwd#expire` claimed on Linux,
/// `Process::CLOCK_UPTIME_RAW` on both). A fold miss only degrades to runtime
/// dispatch, but a fold HIT on an absent row is an answer ruby does not give.
///
/// Cargo hands a build script the target's own configuration, and today the
/// target IS the host, so this makes the surface exact. When `--target`
/// arrives (G12) the same predicate reads `TargetSpec` instead, in this one
/// place.
fn cfg_holds(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .all(|a| match a.parse_args::<syn::Meta>() {
            Ok(m) => eval_cfg(&m),
            Err(e) => panic!("a ruby_class! row has a #[cfg] this build cannot read: {e}"),
        })
}

/// One `cfg` predicate. Anything unrecognized PANICS rather than answering
/// false: a predicate nobody taught this evaluator would silently delete rows
/// from the surface, which is the bug it exists to fix.
fn eval_cfg(m: &syn::Meta) -> bool {
    match m {
        // A bare flag (`unix`, `windows`). Cargo sets `CARGO_CFG_UNIX` to the
        // empty string when it holds and omits it otherwise, so presence is
        // the answer.
        syn::Meta::Path(p) => {
            let name = last_ident(p);
            match name.as_str() {
                "unix" | "windows" => {
                    std::env::var_os(format!("CARGO_CFG_{}", name.to_uppercase())).is_some()
                }
                // The projection describes the SHIPPED surface, so a `#[cfg(test)]`
                // module never contributes to it -- including one that invokes
                // `ruby_class!` to build a fixture.
                "test" => false,
                other => panic!("a ruby_class! row is gated on an unknown cfg flag `{other}`"),
            }
        }
        syn::Meta::List(l) => {
            let name = last_ident(&l.path);
            let inner = l
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                )
                .unwrap_or_else(|e| panic!("a ruby_class! row has a malformed cfg `{name}`: {e}"));
            match name.as_str() {
                "not" => !inner.iter().all(eval_cfg),
                "any" => inner.iter().any(eval_cfg),
                "all" => inner.iter().all(eval_cfg),
                other => panic!("a ruby_class! row is gated on an unknown cfg operator `{other}`"),
            }
        }
        syn::Meta::NameValue(nv) => {
            let key = last_ident(&nv.path);
            let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
            else {
                panic!("a ruby_class! row's cfg `{key}` is not compared against a string");
            };
            let want = s.value();
            // A `feature` on a builtin row names one of `zeo-rt`'s `ext-*`
            // features, and this build script reads `zeo-rt`'s SOURCE rather
            // than its resolved feature set. Every one of them is in the
            // shipped `ext-all` default, so they hold; a build that disables
            // an ext gets an over-approximate surface for that ext alone,
            // which is what every row got before this function existed.
            if key == "feature" {
                return true;
            }
            let var = format!("CARGO_CFG_{}", key.to_uppercase());
            let Ok(have) = std::env::var(&var) else {
                panic!("a ruby_class! row is gated on `{key}`, which cargo does not set as {var}");
            };
            // Multi-valued keys (`target_family`) arrive comma-separated.
            have.split(',').any(|v| v == want)
        }
    }
}

fn last_ident(path: &syn::Path) -> String {
    path.segments
        .last()
        .expect("a cfg predicate has at least one segment")
        .ident
        .to_string()
}

/// Render a name list as Rust `"a", "b"` string-literal elements.
fn str_list(items: &[String]) -> String {
    items
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every `ruby_class!`/`ruby_module!` header's ID CONST, from the whole tree.
///
/// A light scan rather than the DSL parse `collect_from_dir` runs: the id is
/// all this list needs, and the strict parse rejects rows gated on a cfg it
/// does not know (`debug_assertions`) in files outside `builtins/` and `ext/`.
fn collect_table_ids(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(collect_table_ids(&path));
            continue;
        }
        if !path.extension().is_some_and(|e| e == "rs") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Both the outer header (`ruby_class! { Array = zeo_abi::ARRAY_CLASS`)
        // and a NESTED one (`class Port = zeo_abi::RACTOR_PORT_CLASS <`),
        // which the macro expands into a submodule with a table of its own.
        for line in src.lines() {
            let line = line.trim_start();
            let after_eq = if let Some(rest) = line.strip_prefix("class ") {
                rest
            } else if let Some(rest) = line.strip_prefix("module ") {
                rest
            } else {
                line
            };
            let Some(eq) = after_eq.find('=') else {
                continue;
            };
            // The text before `=` must be exactly the class's name. Without
            // that, `let consts = zeo_abi::SOCKET_CONSTANTS_MODULE` and
            // `if id != zeo_abi::OBJECT_CLASS` both read as headers, and a
            // phantom name here is a LINK error in every emitted program.
            let name = after_eq[..eq].trim();
            if name.is_empty()
                || !name.chars().all(|c| c.is_alphanumeric() || c == '_')
                || after_eq[eq + 1..].starts_with('=')
            {
                continue;
            }
            let token: String = after_eq[eq + 1..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
                .collect();
            let last = token.rsplit("::").next().unwrap_or_default();
            if (last.ends_with("_CLASS") || last.ends_with("_MODULE"))
                && token.starts_with("zeo_abi::")
            {
                out.push(last.to_string());
            }
        }
    }
    out
}

fn collect_from_dir(dir: &Path, out: &mut Vec<Surface>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // A gem namespaces its classes in a subdirectory (`ext/socket/`).
            collect_from_dir(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            // Re-run when any source changes (a migrated header edited, a class
            // newly migrated).
            println!("cargo:rerun-if-changed={}", path.display());
            collect_from_file(&path, out);
        }
    }
}

fn collect_from_file(path: &Path, out: &mut Vec<Surface>) {
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    // A file that doesn't parse as Rust can't host a macro invocation we
    // understand; skip rather than fail the whole build.
    let Ok(file) = syn::parse_file(&source) else {
        return;
    };
    collect_from_items(&file.items, out);
}

/// Walk items (recursing into inline `mod` blocks) for `ruby_class!`/
/// `ruby_module!` invocations.
fn collect_from_items(items: &[syn::Item], out: &mut Vec<Surface>) {
    for item in items {
        match item {
            syn::Item::Macro(m) if cfg_holds(&m.attrs) => surface_from_macro(m, out),
            syn::Item::Mod(m) if cfg_holds(&m.attrs) => {
                if let Some((_, inner)) = &m.content {
                    collect_from_items(inner, out);
                }
            }
            _ => {}
        }
    }
}

fn surface_from_macro(m: &syn::ItemMacro, out: &mut Vec<Surface>) {
    let Some(macro_name) = m.mac.path.segments.last().map(|s| s.ident.to_string()) else {
        return;
    };
    let tokens = m.mac.tokens.clone();
    let spec = match macro_name.as_str() {
        "ruby_class" => ClassSpec::parse_class.parse2(tokens).ok(),
        "ruby_module" => ClassSpec::parse_module.parse2(tokens).ok(),
        _ => None,
    };
    if let Some(spec) = spec {
        surface_from_spec(&spec, out);
    }
}

/// The trailing segment of a `ClassId` path (`zeo_abi::STRING_CLASS` ->
/// `STRING_CLASS`), which is what the generated table re-qualifies.
fn const_name(path: &syn::Path) -> String {
    path.segments
        .last()
        .expect("a ClassId path has at least one segment")
        .ident
        .to_string()
}

fn surface_from_spec(spec: &ClassSpec, out: &mut Vec<Surface>) {
    let mut instance_methods = Vec::new();
    let mut class_methods = Vec::new();
    for method in &spec.methods {
        // A row this target does not compile is not surface -- see `cfg_holds`.
        if !cfg_holds(&method.attrs) {
            continue;
        }
        for name in &method.names {
            // `module_function def` defines the method BOTH ways, exactly as
            // CRuby's `module_function` does -- so `Math.sqrt` is a class
            // method AND `sqrt` an instance method of anything that includes
            // `Math`. Bucketing on `is_class_method` alone lost all 39 of them.
            if method.is_class_method || method.is_module_function {
                class_methods.push(name.ruby.clone());
            }
            if !method.is_class_method {
                instance_methods.push(name.ruby.clone());
            }
        }
    }
    // A late alias joins the bucket of the method it aliases.
    for alias in &spec.aliases {
        if class_methods.iter().any(|n| n == &alias.old_name) {
            class_methods.push(alias.new_name.clone());
        } else {
            instance_methods.push(alias.new_name.clone());
        }
    }

    out.push(Surface {
        id_const: const_name(&spec.id),
        header_name: spec.name.to_string(),
        superclass: match &spec.kind {
            zeo_dsl::ClassKind::Class { superclass } => superclass.as_ref().map(const_name),
            zeo_dsl::ClassKind::Module => None,
        },
        is_module: matches!(spec.kind, zeo_dsl::ClassKind::Module),
        includes: spec.includes.iter().map(const_name).collect(),
        instance_methods,
        class_methods,
        constants: spec
            .consts
            .iter()
            .filter(|c| cfg_holds(&c.attrs))
            .map(|c| c.name.to_string())
            .collect(),
    });

    // A nested `class Status = ... { .. }` is a class in its own right, with
    // its own ClassId and members. Without this it got no surface row at all,
    // so `Process::Status` and `Process::Tms` folded blind.
    for nested in &spec.nested {
        surface_from_spec(nested, out);
    }
}

/// Publish the C API so a loaded extension can resolve it.
///
/// An extension's `.so` leaves every `rb_*` undefined and resolves it against
/// the host at load. Two things stop that by default, and both need saying
/// because each fails differently:
///
/// * Nothing in `zeo` CALLS `rb_raise`, so the linker never pulls that
///   archive member in. The C-API crate marks its own C archive
///   `+whole-archive`, and Apple's `-all_load` takes every member of every
///   archive on the line besides.
/// * An executable's dynamic symbol table holds only what was asked for.
///   `-export_dynamic` publishes the rest, which is what `dlsym` reads.
///
/// Without the first the symbol is absent; without the second it is present
/// and invisible. Either way the extension's first call jumps to nothing --
/// which is a SIGSEGV inside `Init_`, with no diagnostic at all.
///
/// CRuby links its own interpreter the same way, for the same reason.
///
/// The two spellings are NOT interchangeable, and getting it wrong is worse
/// than a missing flag: GNU ld reads `-export_dynamic` as `-e xport_dynamic`,
/// because `-e` takes the entry symbol and the rest of the word is its
/// argument. It warns, picks an arbitrary entry point, and links a binary
/// that starts in the wrong place -- which surfaced as a `capacity overflow`
/// panic from a thread with no name, nowhere near any zeo code.
/// `zeo file.rb --backend jit` runs the compiled program on THIS binary's
/// main thread (`zeo_rt::exec::run_main`: on macOS the top level runs on
/// the process main thread, the one AppKit demands), so the driver's main
/// thread needs the stack an AOT binary's link line gives it -- 64 MiB,
/// `zeo_rt::MAIN_STACK_SIZE`, which `backend::link::main_stack_link_arg`
/// spells the same way (a test there pins this literal to it). Linux sizes
/// a spawned thread instead; an ELF has no such load command.
fn size_the_main_thread_stack() {
    if cfg!(target_vendor = "apple") {
        println!("cargo:rustc-link-arg-bins=-Wl,-stack_size,0x4000000");
    }
}

fn export_cext_surface() {
    if cfg!(target_vendor = "apple") {
        println!("cargo:rustc-link-arg-bins=-Wl,-export_dynamic");
        // `-all_load` rather than `-force_load,<path>`: cargo does not tell a
        // build script where `libzeo.a` will land, and every other archive on
        // the line is a Rust dependency whose members were already selected.
        println!("cargo:rustc-link-arg-bins=-Wl,-all_load");
    } else {
        println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic");
    }
}
