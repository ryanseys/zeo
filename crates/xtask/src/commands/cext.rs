//! Manage the vendored MRI C API headers under `crates/zeo-rt/cext/`.
//!
//! ```text
//! cext sync [--check]
//! cext patch <name>
//! cext api [--check]
//! cext forward [--check|--reverify]
//! ```
//!
//! zeo is source-compatible with MRI and ABI-incompatible with it: a gem's
//! `ext/**/*.c` compiles against MRI's own headers, and a prebuilt MRI `.so`
//! never loads. The headers are therefore upstream verbatim plus a patch
//! series that turns every layout-reading macro into a call, because a zeo
//! heap object is an opaque handle and has no `struct RString` behind it.
//!
//! `sync` rebuilds the tree from that sum, so the vendored bytes are always
//! exactly `upstream(rev) + patches/`. `--check` proves it without writing,
//! which is what CI runs -- a hand-edit to a vendored header is drift, and
//! the way to keep one is `cext patch`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::exec::{self, Capture};
use crate::scratch::Scratch;
use crate::{Error, root, root_join, vendor, write_if_changed};

const USAGE: &str = "\
usage: cargo xtask cext <subcommand> [options]

subcommands:
  sync [--check]      rebuild the headers from upstream + patches/
  patch <name>        record the working tree's deviation as a patch
  api [--check]       re-record the rb_* census and regenerate stubs
  forward [--check|--reverify]
                      the rb_* -> Class#method forwarding table
";

const CEXT: &str = "crates/zeo-rt/cext";
const CEXT_SRC: &str = "crates/zeo-rt/src/cext";
const API_RS: &str = "crates/zeo-rt/src/cext/api.rs";
const STUBS_RS: &str = "crates/zeo-rt/src/cext/stubs.rs";
const FORWARD_RS: &str = "crates/zeo-rt/src/cext/forward.rs";
const MKMF_RB: &str = "crates/zeo/tools-lib/mkmf.rb";

fn include_dir() -> PathBuf {
    root_join(CEXT).join("include")
}

fn patch_dir() -> PathBuf {
    root_join(CEXT).join("patches")
}

fn patches() -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(patch_dir()) else {
        return Ok(out);
    };
    for entry in entries {
        let path = entry
            .map_err(|e| Error::new(format!("reading {}: {e}", patch_dir().display())))?
            .path();
        if path.extension().is_some_and(|e| e == "patch") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

pub fn run(args: &[String]) -> Result<(), Error> {
    let mut sub = None;
    let mut name = None;
    let mut check = false;
    let mut reverify = false;
    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            "--reverify" => reverify = true,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(Error::new(format!("unknown option {other:?}\n\n{USAGE}")));
            }
            other if sub.is_none() => sub = Some(other.to_string()),
            other => name = Some(other.to_string()),
        }
    }
    match sub.as_deref() {
        Some("sync") => cmd_sync(check),
        Some("patch") => cmd_patch(name),
        Some("api") => cmd_api(check),
        Some("forward") => cmd_forward(check, reverify),
        _ => Err(Error::new(USAGE.to_string())),
    }
}

// ---------------------------------------------------------------- sync/patch

/// Upstream's `include/` at the pinned rev, with the patch series applied on
/// top, materialized in a scratch directory.
fn build_expected(dest: &Path) -> Result<(), Error> {
    let pin = vendor::Pin::ruby_headers()?;
    let checkout = vendor::fetch_checkout(&pin)?;
    vendor::remove_dir_all(dest)?;
    vendor::copy_tree(&pin.source_root(&checkout), dest)?;
    for patch in patches()? {
        let out = exec::run(
            &[
                Path::new("git"),
                Path::new("apply"),
                Path::new("--whitespace=nowarn"),
                &patch,
            ],
            dest,
            &[],
            Capture::Both,
        )?;
        if !out.success() {
            return Err(Error::new(format!(
                "{} does not apply to upstream {}",
                patch.file_name().unwrap_or_default().to_string_lossy(),
                pin.tag
            )));
        }
    }
    Ok(())
}

fn cmd_sync(check: bool) -> Result<(), Error> {
    let pin = vendor::Pin::ruby_headers()?;
    let tmp = Scratch::new("cext")?;
    let want = tmp.path().join("include");
    build_expected(&want)?;
    let include = include_dir();

    if check {
        if !vendor::dirs_equal(&want, &include)? {
            return Err(report_drift(&want, &include, &pin.tag)?);
        }
        let mkmf = root_join(MKMF_RB);
        if std::fs::read_to_string(&mkmf).ok().as_deref() != Some(&upstream_mkmf(&pin)?) {
            return Err(Error::new(format!(
                "{MKMF_RB} is not upstream {}'s lib/mkmf.rb",
                pin.tag
            )));
        }
        println!(
            "cext: {} headers match upstream {} + {} patch(es), and mkmf.rb matches",
            vendor::list_files(&want)?.len(),
            pin.tag,
            patches()?.len()
        );
        return Ok(());
    }

    vendor::remove_dir_all(&include)?;
    vendor::copy_tree(&want, &include)?;
    std::fs::write(root_join(MKMF_RB), upstream_mkmf(&pin)?)
        .map_err(|e| Error::new(format!("writing {MKMF_RB}: {e}")))?;
    println!(
        "cext: vendored {} headers from {} @ {} + {} patch(es)",
        vendor::list_files(&include)?.len(),
        pin.repo,
        pin.tag,
        patches()?.len()
    );
    Ok(())
}

/// `lib/mkmf.rb` is vendored VERBATIM: it is 3,061 lines of Ruby that zeo
/// runs rather than reimplements, and a local edit to it would be a
/// divergence nobody could see. It rides the same pin as the headers.
fn upstream_mkmf(pin: &vendor::Pin) -> Result<String, Error> {
    let checkout = vendor::fetch_checkout(pin)?;
    let path = checkout.join("lib/mkmf.rb");
    std::fs::read_to_string(&path).map_err(|e| Error::new(format!("reading {}: {e}", path.display())))
}

/// Name every file that differs, not just the count: a header tree is too big
/// for a bare "drift" to be actionable.
fn report_drift(want: &Path, include: &Path, tag: &str) -> Result<Error, Error> {
    let expected = vendor::list_files(want)?;
    let have = vendor::list_files(include)?;
    for rel in expected.iter().filter(|r| !have.contains(r)) {
        eprintln!("  missing: {rel}");
    }
    for rel in have.iter().filter(|r| !expected.contains(r)) {
        eprintln!("  extra:   {rel}");
    }
    for rel in expected.iter().filter(|r| have.contains(r)) {
        if std::fs::read(want.join(rel)).ok() != std::fs::read(include.join(rel)).ok() {
            eprintln!("  changed: {rel}");
        }
    }
    Ok(Error::new(format!(
        "the vendored headers are not upstream {tag} + patches/ -- run \
         `cargo xtask cext patch <name>` to keep an edit, or `cext sync` to discard it"
    )))
}

/// Fold the working tree's whole deviation into one new patch. The series is
/// applied in name order, so a later patch may depend on an earlier one;
/// recording the deviation as a single hunk set keeps that honest.
fn cmd_patch(name: Option<String>) -> Result<(), Error> {
    let name =
        name.ok_or_else(|| Error::new("cext patch needs a name, e.g. `rstring-is-opaque`"))?;
    let tmp = Scratch::new("cext")?;
    let want = tmp.path().join("include");
    build_expected(&want)?;
    let include = include_dir();
    if vendor::dirs_equal(&want, &include)? {
        println!("cext: nothing to record -- the tree already matches upstream + patches/");
        return Ok(());
    }
    let diff = vendor::diff_trees(&want, &include)?;
    let out = patch_dir().join(format!("{:04}-{name}.patch", patches()?.len() + 1));
    std::fs::create_dir_all(patch_dir())
        .map_err(|e| Error::new(format!("creating {}: {e}", patch_dir().display())))?;
    // `git apply` ignores anything before the first `diff --git`, so the patch
    // carries its own reason. A headerless one says nothing about WHY a
    // vendored header reads the way it does.
    let header = format!(
        "Subject: {}\n\nTODO: say what this changes and why.\n\n",
        name.replace('-', " ")
    );
    std::fs::write(&out, header + &rewrite_prefixes(&diff, &want, &include))
        .map_err(|e| Error::new(format!("writing {}: {e}", out.display())))?;
    println!(
        "cext: wrote {}",
        out.strip_prefix(root()).unwrap_or(&out).display()
    );
    Ok(())
}

/// `--no-index` writes absolute scratch paths into the header lines. A patch
/// that names a tmpdir cannot be re-applied, so rewrite both sides to the
/// plain relative paths `git apply` expects inside the tree.
fn rewrite_prefixes(text: &str, want: &Path, include: &Path) -> String {
    let want = want.display().to_string();
    let include = include.display().to_string();
    text.replace(&format!("a{want}/"), "a/")
        .replace(&format!("b{include}/"), "b/")
        .replace(&format!("{want}/"), "")
        .replace(&format!("{include}/"), "")
}

// ----------------------------------------------------------------------- api

/// One declaration clang found in the vendored headers.
struct Decl {
    name: String,
    kind: &'static str,
    sig: String,
    status: &'static str,
}

/// Every public header a gem may include, not just what `<ruby.h>` pulls in
/// transitively.
///
/// This was `#include <ruby.h>` alone, and the gap was not academic:
/// `ruby/encoding.h` is included by `fast_blank`, `rb_enc_codepoint_len` was
/// therefore absent from the census, no stub was generated for it, and the
/// gem linked and then SIGSEGVd on its first call. A symbol the census cannot
/// see is a symbol nothing promises.
///
/// The Windows and Oniguruma headers are excluded: `win32.h` does not parse
/// on a POSIX host, and `onigmo.h`/`oniguruma.h`/`regex.h` declare the regexp
/// ENGINE's own surface, which zeo answers with its own onig build rather
/// than through the C API.
const SKIP_HEADERS: &[&str] = &["win32.h", "onigmo.h", "oniguruma.h", "regex.h"];

fn public_headers() -> Result<Vec<String>, Error> {
    let dir = include_dir().join("ruby");
    let entries =
        std::fs::read_dir(&dir).map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".h") && !SKIP_HEADERS.contains(&name.as_str()) {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

/// Every `rb_*`/`ruby_*` an extension can link against, from clang's own AST
/// rather than a regex over the headers.
///
/// A regex cannot tell `RSTRING_LEN` -- a `static inline` an extension
/// compiles into its own object -- from `rb_str_new`, which it expects to
/// find at link time. Only the second kind needs an implementation or a stub,
/// and clang already knows which is which.
fn scan_api() -> Result<Vec<Decl>, Error> {
    let tmp = Scratch::new("cext-api")?;
    let tu = tmp.path().join("all.c");
    let mut source = String::from("#include <ruby.h>\n");
    for header in public_headers()? {
        source.push_str(&format!("#include <ruby/{header}>\n"));
    }
    std::fs::write(&tu, source)
        .map_err(|e| Error::new(format!("writing {}: {e}", tu.display())))?;
    // `clang` by name, not `cc`: `-Xclang -ast-dump` is clang's own flag, and
    // `cc` is gcc on Linux, which rejects it and dumps no AST.
    let cc = std::env::var("CC").unwrap_or_else(|_| "clang".into());
    let out = exec::run(
        &[
            Path::new(&cc),
            Path::new("-fsyntax-only"),
            Path::new("-Xclang"),
            Path::new("-ast-dump"),
            Path::new("-fno-color-diagnostics"),
            Path::new("-I"),
            &root_join(CEXT).join("config"),
            Path::new("-I"),
            &include_dir(),
            &tu,
        ],
        root(),
        &[],
        Capture::Both,
    )?;
    if out.stdout.is_empty() {
        return Err(Error::new(format!(
            "{cc} could not parse the vendored headers:\n{}",
            out.stderr_text().trim_end()
        )));
    }
    parse_ast(&out.stdout_text())
}

/// A linkable declaration is one clang did NOT mark `static inline`: that
/// marker is the whole difference between a macro-shaped helper the gem
/// compiles itself and a symbol it expects zeo to provide.
///
/// `rbimpl_zeo_*` is in the set because the PATCH introduces those, and the
/// promise is that every DECLARED symbol resolves -- not every one upstream
/// declares. `RTYPEDDATA_DATA` expands to `rbimpl_zeo_data_slot`, which was
/// invisible here and defined nowhere, so msgpack linked and failed at load.
fn parse_ast(text: &str) -> Result<Vec<Decl>, Error> {
    let name = r"((?:rb|ruby|rbimpl_zeo)_\w+)";
    let head = r"0x[0-9a-f]+ (?:prev 0x[0-9a-f]+ )?<[^>]*> (?:line|col):\S+ (?:used |referenced )?";
    let fn_re = Regex::new(&format!(r"FunctionDecl {head}{name} '([^']*)'\s*$")).expect("a valid pattern");
    let var_re = Regex::new(&format!(
        r"VarDecl {head}{name} '([^']*)'(?::'[^']*')? extern\s*$"
    ))
    .expect("a valid pattern");

    let mut seen: BTreeMap<String, Decl> = BTreeMap::new();
    for line in text.lines() {
        let (caps, kind) = match fn_re.captures(line) {
            Some(caps) => (caps, "fn"),
            None => match var_re.captures(line) {
                Some(caps) => (caps, "var"),
                None => continue,
            },
        };
        let name = caps[1].to_string();
        seen.entry(name.clone()).or_insert(Decl {
            name,
            kind,
            sig: caps[2].to_string(),
            status: "",
        });
    }

    let implemented = implemented()?;
    let mut decls: Vec<Decl> = seen.into_values().collect();
    for d in &mut decls {
        d.status = if implemented.contains(&d.name) {
            "zeo"
        } else if d.kind == "var" {
            // A global is always a real symbol; what varies is whether the
            // loader can fill it. `cext::globals` reports the ones it cannot.
            "global"
        } else if refused(&d.name).is_some() {
            "refused"
        } else {
            "stub"
        };
    }
    Ok(decls)
}

/// Symbols the runtime already exports, read off its own `#[no_mangle]`
/// attributes. Asking the source rather than keeping a list is what stops the
/// two drifting.
fn implemented() -> Result<Vec<String>, Error> {
    // Two spellings reach the same place: a hand-written export, and the
    // `cext_fn!` macro that wraps a `Result` body into one.
    let exported = Regex::new(
        r#"#\[unsafe\(no_mangle\)\]\s*(?:pub\s+)?(?:unsafe\s+)?extern "C" fn (\w+)"#,
    )
    .expect("a valid pattern");
    let wrapped =
        Regex::new(r"(?m)^\s*fn (rb_\w+|ruby_\w+|rbimpl_zeo_\w+)\s*\(").expect("a valid pattern");
    // The variadic entries live in `csrc/*.c`, because Rust cannot read a
    // `va_list`. A definition there is as real as one in Rust, and missing it
    // would leave a duplicate symbol at link time.
    let in_c = Regex::new(r"(?m)^(?:\w[\w *]*?)\b((?:rb|ruby|st|rbimpl_zeo)_\w+)\s*\([^;]*$")
        .expect("a valid pattern");

    let mut out = Vec::new();
    for path in files_with_extension(&root_join(CEXT_SRC), "rs")? {
        // `stubs.rs` is this command's own output. Counting its stubs as
        // implementations would make the second run report every one of them
        // as done.
        if path.file_name().is_some_and(|n| n == "stubs.rs") {
            continue;
        }
        let src = read(&path)?;
        out.extend(exported.captures_iter(&src).map(|c| c[1].to_string()));
        out.extend(wrapped.captures_iter(&src).map(|c| c[1].to_string()));
    }
    for path in files_with_extension(&root_join("crates/zeo-rt/csrc"), "c")? {
        let src = read(&path)?;
        out.extend(in_c.captures_iter(&src).map(|c| c[1].to_string()));
    }
    Ok(out)
}

fn cmd_api(check: bool) -> Result<(), Error> {
    let decls = scan_api()?;
    let api = rustfmt_would_write(&render_api_rs(&decls))?;
    let stubs = rustfmt_would_write(&render_stubs(&decls))?;
    let stubbed = decls.iter().filter(|d| d.status == "stub").count();
    let refused = decls.iter().filter(|d| d.status == "refused").count();

    if check {
        let drift: Vec<&str> = [(API_RS, &api), (STUBS_RS, &stubs)]
            .into_iter()
            .filter(|(path, want)| {
                std::fs::read(root_join(path)).ok().as_deref() != Some(want.as_bytes())
            })
            .map(|(path, _)| path)
            .collect();
        if !drift.is_empty() {
            return Err(Error::new(format!(
                "stale -- run `cargo xtask cext api`: {}",
                drift.join(", ")
            )));
        }
        println!(
            "cext: {} linkable symbols, {stubbed} stubbed, {refused} refused",
            decls.len()
        );
        return Ok(());
    }
    write_if_changed(&root_join(API_RS), api.as_bytes())?;
    write_if_changed(&root_join(STUBS_RS), stubs.as_bytes())?;
    println!(
        "cext: wrote {API_RS} and {STUBS_RS} ({} symbols, {stubbed} stubbed, {refused} refused)",
        decls.len()
    );
    Ok(())
}

/// What `rustfmt` makes of a rendering, so `--check` compares like with like.
fn rustfmt_would_write(text: &str) -> Result<String, Error> {
    let tmp = Scratch::new("cext-fmt")?;
    let path = tmp.path().join("stubs.rs");
    std::fs::write(&path, text)
        .map_err(|e| Error::new(format!("writing {}: {e}", path.display())))?;
    let out = exec::run(
        &[Path::new("rustfmt"), Path::new("--edition"), Path::new("2024"), &path],
        root(),
        &[],
        Capture::Both,
    )?;
    // A toolchain without the rustfmt component fails here, not later as a
    // 3,000-line "stale" diff against the formatted committed tables.
    if !out.success() {
        return Err(Error::new(format!(
            "rustfmt failed (is the component installed?):\n{}",
            out.stderr_text().trim_end()
        )));
    }
    read(&path)
}

/// Entries zeo will NOT implement, each with the reason.
///
/// The distinction from `stub` is the whole point: a stub is work not done,
/// and the count going down is progress. A refusal is a decision, and it
/// stays. Without the split, 33 permanent refusals read as 33 outstanding
/// items forever.
///
/// The message an extension sees carries the reason, so a gem that calls one
/// gets an answer rather than a bare "not implemented".
fn refused(name: &str) -> Option<&'static str> {
    const EMBEDDING: &str = "zeo's VM is already running: these boot, configure or shut down an \
                             interpreter, and an extension loaded INTO one cannot do that";
    const BIGNUM_LAYOUT: &str = "this exposes MRI's Bignum digit array, which zeo does not have; \
                                 rb_integer_pack and rb_integer_unpack are the supported way";
    const HASH_TABLE: &str = "this hands out a Ruby Hash's internal st_table, which zeo does not \
                              store; rb_hash_foreach and rb_hash_aset reach the same data";
    const PARSE_TREE: &str = "this answers a NODE*, MRI's parse tree; zeo compiles through prism \
                              and builds no such thing";
    Some(match name {
        "ruby_init" | "ruby_setup" | "ruby_cleanup" | "ruby_finalize" | "ruby_sig_finalize"
        | "ruby_stop" | "ruby_options" | "ruby_process_options" | "ruby_prog_init"
        | "ruby_sysinit" | "ruby_init_loadpath" | "ruby_init_stack" | "ruby_incpush"
        | "ruby_script" | "ruby_set_argv" | "ruby_set_script_name" | "ruby_show_copyright"
        | "ruby_show_version" | "ruby_run_node" | "ruby_exec_node" | "ruby_executable_node" => {
            EMBEDDING
        }
        "rb_big_new" | "rb_big_resize" | "rb_big_pack" | "rb_big_unpack" | "rb_big_2comp" => {
            BIGNUM_LAYOUT
        }
        "rb_hash_tbl" | "rb_hash_bulk_insert_into_st_table" => HASH_TABLE,
        "rb_load_file" | "rb_load_file_str" => PARSE_TREE,
        "rb_add_event_hook" => {
            "the C-level TracePoint; zeo's TracePoint is Ruby-level and its event set does not \
             line up with rb_event_flag_t"
        }
        "rb_remove_event_hook" => "the C-level TracePoint; see rb_add_event_hook",
        "rb_marshal_define_compat" => {
            "this writes Marshal's internal compatibility table, which zeo's Marshal does not have"
        }
        _ => return None,
    })
}

fn render_api_rs(decls: &[Decl]) -> String {
    let rows: String = decls
        .iter()
        .map(|d| {
            format!(
                "    ({:?}, {:?}, {:?}, {:?}),\n",
                d.name, d.kind, d.status, d.sig
            )
        })
        .collect();
    format!(
        r##"//! Every `rb_*`/`ruby_*` symbol a C extension can link against.
//!
//! Generated by `cargo xtask cext api` from clang's own AST of the vendored
//! headers. Do not edit.
//!
//! A regex over the headers cannot tell `RSTRING_LEN` -- a `static inline` an
//! extension compiles into its own object -- from `rb_str_new`, which it
//! expects to find at LINK time. Only the second kind needs an implementation
//! or a stub, and clang already knows which is which.
//!
//! This is committed Rust rather than a data file so that a fresh clone
//! builds and every row is reviewable in a diff. The unit tests in
//! `cext::tests` read it back and check it still describes the runtime.
//!
//! | status | meaning |
//! |---|---|
//! | `zeo` | the runtime exports it |
//! | `stub` | not written yet; `stubs.rs` raises `NotImplementedError` |
//! | `refused` | a DECISION, with the reason in the raise -- see `refused` in `crates/xtask/src/commands/cext.rs` |
//! | `global` | a `VALUE` symbol `stubs.rs` defines and `cext::globals` fills |

/// `(symbol, kind, status, signature)`, sorted by symbol.
///
/// `cfg(test)`: the record is read by `cext::tests` and by the generator,
/// never by the runtime, so a release build carries none of these strings.
#[cfg(test)]
pub(crate) const API: &[(&str, &str, &str, &str)] = &[
{rows}];
"##
    )
}

fn render_stubs(decls: &[Decl]) -> String {
    let of = |kind: &str, status: &str| -> Vec<&Decl> {
        decls
            .iter()
            .filter(|d| d.kind == kind && d.status == status)
            .collect()
    };
    let stubs = of("fn", "stub");
    let refusals = of("fn", "refused");
    let vars = of("var", "global");

    let table: String = vars
        .iter()
        .map(|d| format!("    ({:?}, &{}),\n", d.name, d.name))
        .collect();
    let globals: String = vars
        .iter()
        .map(|d| format!("#[unsafe(no_mangle)]\npub static {}: Global = unfilled();\n", d.name))
        .collect();
    let stub_fns: String = stubs
        .iter()
        .map(|d| {
            format!(
                "#[unsafe(no_mangle)]\npub extern \"C\" fn {}() -> ! {{\n    unimplemented({:?})\n}}\n",
                d.name, d.name
            )
        })
        .collect();
    let refused_fns: String = refusals
        .iter()
        .map(|d| {
            format!(
                "#[unsafe(no_mangle)]\npub extern \"C\" fn {}() -> ! {{\n    refused({:?}, {:?})\n}}\n",
                d.name,
                d.name,
                refused(&d.name).expect("a refused decl carries a reason")
            )
        })
        .collect();
    // `unimplemented` exists only while something still uses it. The count is
    // zero today, and a re-vendor at a later Ruby is what brings new symbols
    // and needs it back.
    let unimplemented_fn = if stubs.is_empty() {
        String::new()
    } else {
        "/// Raise, naming the symbol the extension asked for.\n\
         fn unimplemented(what: &'static str) -> ! {\n    \
             crate::cext::jmp::raise(crate::builtins::not_impl_error!(\n        \
                 \"{what} is not implemented by zeo\"\n    \
             ))\n\
         }\n"
            .to_string()
    };

    format!(
        r##"//! One loud stub per `rb_*` zeo does not answer yet.
//!
//! Generated by `cargo xtask cext api`. Do not edit.
//!
//! # Why every one of them exists
//!
//! Two kinds live here. A STUB is work not done, and the count going down is
//! progress. A REFUSAL is a decision that stays, and it carries its reason
//! into the raise -- so a gem author reading the message learns what to reach
//! for instead.
//!
//! A gem's `ext/**/*.c` is compiled and linked as a whole. One reference to a
//! function zeo has not written yet would fail the LINK, with a message
//! naming a symbol and no gem, no file and no line -- and it would fail even
//! when the call sits on a branch the program never takes. So every declared
//! symbol resolves, and one that is not implemented raises when it is CALLED,
//! naming itself.
//!
//! A stub disappears the moment the real function is written: the generator
//! reads `#[unsafe(no_mangle)]` out of the other files in this module, so
//! implementing one is all it takes.
//!
//! # The signatures
//!
//! A stub takes no arguments and never returns. By the C standard that is a
//! prototype mismatch; on both ABIs zeo targets it is safe. The caller passes
//! arguments in registers and on a stack it cleans up itself, the callee
//! reads none of them, and it never returns -- so there is no return value to
//! disagree about and no frame to unwind. Writing {n_stubs} correct
//! signatures would buy nothing: not one of these functions runs. {n_refused}
//! of them are refusals rather than gaps.
//!
//! # The globals
//!
//! `rb_cObject`, `rb_eArgError` and {n_other_globals} others are `VALUE`
//! variables an extension reads directly, so each is a real symbol rather
//! than a stub. Each starts as `Qundef` and the loader fills it before any
//! `Init_` runs; [`GLOBALS`] is what it walks, and `every_global_is_filled`
//! is what proves none was missed. Nothing can read one before the loader
//! runs, because nothing has loaded.

use std::sync::atomic::{{AtomicUsize, Ordering}};

/// A `VALUE` variable C reads by name. `AtomicUsize` so Rust can write it; C
/// sees a plain `VALUE`, which is the same word.
pub type Global = AtomicUsize;

/// Every global starts here, and the loader is what moves it.
const fn unfilled() -> Global {{
    AtomicUsize::new(crate::cext::value::Q_UNDEF)
}}

/// Every `VALUE` global, by the name C spells.
pub static GLOBALS: &[(&str, &Global)] = &[
{table}];

/// Read one, for a caller that has the name rather than the symbol.
pub fn global(name: &str) -> Option<usize> {{
    GLOBALS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, g)| g.load(Ordering::Relaxed))
}}

{unimplemented_fn}
/// The same, for an entry zeo has DECIDED not to implement. The reason
/// travels with the raise, so a gem author reading the message learns what to
/// reach for instead.
fn refused(what: &'static str, why: &'static str) -> ! {{
    crate::cext::jmp::raise(crate::builtins::not_impl_error!(
        "{{what}} is not supported by zeo: {{why}}"
    ))
}}

{globals}
{stub_fns}{refused_fns}"##,
        n_stubs = stubs.len(),
        n_refused = refusals.len(),
        n_other_globals = vars.len().saturating_sub(2),
    )
}

// ------------------------------------------------------------------- forward

/// One verified `rb_* == Class#method` claim.
struct Fwd {
    name: String,
    klass: String,
    meth: String,
    nargs: usize,
}

/// `rb_<prefix>_<rest>` names the class the prefix stands for. MRI's own
/// convention, and the reason so much of the C API can be forwarded rather
/// than reimplemented.
const PREFIX: &[(&str, &str)] = &[
    ("str", "String"), ("ary", "Array"), ("hash", "Hash"), ("obj", "Object"),
    ("mod", "Module"), ("class", "Class"), ("int", "Integer"), ("big", "Integer"),
    ("num", "Numeric"), ("flo", "Float"), ("float", "Float"), ("sym", "Symbol"),
    ("range", "Range"), ("time", "Time"), ("proc", "Proc"), ("struct", "Struct"),
    ("reg", "Regexp"), ("io", "IO"), ("file", "File"), ("complex", "Complex"),
    ("rational", "Rational"), ("exc", "Exception"), ("dir", "Dir"),
    ("thread", "Thread"), ("mutex", "Thread::Mutex"), ("fiber", "Fiber"),
    ("enum", "Enumerable"), ("set", "Set"), ("method", "Method"),
];

/// MRI spells an operator out in a C name. `_p` is `?` and `_bang` is `!`,
/// both upstream conventions too.
const OPS: &[(&str, &str)] = &[
    ("plus", "+"), ("minus", "-"), ("times", "*"), ("div", "/"), ("modulo", "%"),
    ("pow", "**"), ("cmp", "<=>"), ("aref", "[]"), ("aset", "[]="),
    ("equal", "=="), ("eq", "=="), ("eql", "eql?"), ("lshift", "<<"), ("rshift", ">>"),
    ("and", "&"), ("or", "|"), ("xor", "^"), ("mul", "*"), ("sub", "-"),
    ("idiv", "div"), ("uminus", "-@"), ("uplus", "+@"), ("neg", "-@"),
    ("includes", "include?"), ("size", "size"), ("length", "length"),
];

/// Names whose C meaning is NOT the Ruby method they map onto. Each one was
/// checked by hand against MRI's source, and each would otherwise be a silent
/// wrong answer -- which is the whole reason this list is written out rather
/// than inferred.
const DENY: &[&str] = &[
    // creates a SUBCLASS of its argument; Class#new allocates an instance
    "rb_class_new",
    // answers the `#<Class:0x..>` form for an anonymous class; Class#name answers nil
    "rb_class_name",
    // yields to the C-level block and answers the array; Array#each with no
    // block answers an Enumerator
    "rb_ary_each",
    // same block problem as rb_ary_each
    "rb_hash_delete_if",
    // takes an ARRAY of values; Struct#initialize takes them splatted
    "rb_struct_initialize",
    // takes an ARRAY of arguments; Proc#call takes them splatted
    "rb_proc_call",
    // answers the match POSITION like =~; Regexp#match answers a MatchData
    "rb_reg_match",
];

fn cmd_forward(check: bool, reverify: bool) -> Result<(), Error> {
    let mut rows = if reverify {
        reverify_mappings()?
    } else {
        read_forward_rs()?
    };
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    let rs = rustfmt_would_write(&render_forward_rs(&rows))?;
    if check {
        if read(&root_join(FORWARD_RS))? != rs {
            return Err(Error::new(format!(
                "stale -- run `cargo xtask cext forward`: {FORWARD_RS}"
            )));
        }
        println!("cext: {} forwarded rb_* entries", rows.len());
        return Ok(());
    }
    write_if_changed(&root_join(FORWARD_RS), rs.as_bytes())?;
    println!("cext: wrote {FORWARD_RS} ({} entries)", rows.len());
    Ok(())
}

/// The verified mapping, read back out of the file it generated.
///
/// `forward.rs` carries the claims as a `FORWARDED` table and the bodies that
/// stand on them, so the record and the code cannot drift apart -- there is
/// no data file beside them to fall out of date.
///
/// The scan is over the whole table with the whitespace squeezed, NOT line by
/// line: rustfmt wraps a row past 100 columns onto six lines, and a per-line
/// regex silently dropped the two longest entries.
fn read_forward_rs() -> Result<Vec<Fwd>, Error> {
    let path = root_join(FORWARD_RS);
    if !path.is_file() {
        return Err(Error::new(format!(
            "{FORWARD_RS} is missing -- run `cext forward --reverify`"
        )));
    }
    let text = read(&path)?;
    let head = text
        .find("const FORWARDED")
        .ok_or_else(|| Error::new(format!("{FORWARD_RS} carries no FORWARDED table")))?;
    let end = text[head..]
        .find("];")
        .ok_or_else(|| Error::new(format!("{FORWARD_RS}'s FORWARDED table does not end")))?;
    let table = &text[head..head + end];
    let row = Regex::new(r#"\(\s*"([^"]+)",\s*"([^"]+)",\s*"([^"]+)",\s*(\d+),?\s*\)"#)
        .expect("a valid pattern");
    let rows: Vec<Fwd> = row
        .captures_iter(table)
        .map(|c| Fwd {
            name: c[1].to_string(),
            klass: c[2].to_string(),
            meth: c[3].to_string(),
            nargs: c[4].parse().expect("the pattern matched digits"),
        })
        .collect();
    if rows.is_empty() {
        return Err(Error::new(format!("{FORWARD_RS} carries no FORWARDED rows")));
    }
    Ok(rows)
}

/// Ask the ORACLE which mappings actually hold. Never run in CI: it needs
/// ruby 4.0.6, and the committed table is what CI checks against.
fn reverify_mappings() -> Result<Vec<Fwd>, Error> {
    const SCRIPT: &str = r#"
require "json"
require "set"
out = []
ARGF.each_line do |line|
  name, klass, meth, nargs = line.chomp.split("\t")
  next unless (k = (Object.const_get(klass) rescue nil))
  next unless k.method_defined?(meth) || k.private_method_defined?(meth)
  ar = k.instance_method(meth).arity
  fits = ar >= 0 ? ar == nargs.to_i : (-ar - 1) <= nargs.to_i
  out << { name: name, klass: klass, meth: meth, nargs: nargs.to_i } if fits
end
puts JSON.generate(out)
"#;
    let input: String = candidates()?
        .iter()
        .map(|c| format!("{}\t{}\t{}\t{}\n", c.name, c.klass, c.meth, c.nargs))
        .collect();
    let tmp = Scratch::new("cext-fwd")?;
    let script = tmp.path().join("verify.rb");
    let rows = tmp.path().join("rows.tsv");
    std::fs::write(&script, SCRIPT)
        .map_err(|e| Error::new(format!("writing {}: {e}", script.display())))?;
    // The candidates ride in as an ARGF file rather than on stdin: ARGF reads
    // whatever ARGV names, and a file is one less pipe to keep draining.
    std::fs::write(&rows, input)
        .map_err(|e| Error::new(format!("writing {}: {e}", rows.display())))?;
    let oracle = crate::ruby::Oracle::find();
    let argv = oracle.argv(&[
        "-rset",
        &script.display().to_string(),
        &rows.display().to_string(),
    ]);
    let out = exec::run(&argv, root(), &oracle.env(), Capture::Both)?;
    if !out.success() {
        return Err(Error::new(format!(
            "the oracle could not verify the mappings: {}",
            out.stderr_text()
        )));
    }
    let parsed: Vec<serde_json::Value> = serde_json::from_str(out.stdout_text().trim())
        .map_err(|e| Error::new(format!("the oracle's answer is not JSON: {e}")))?;
    Ok(parsed
        .into_iter()
        .map(|v| Fwd {
            name: v["name"].as_str().unwrap_or_default().to_string(),
            klass: v["klass"].as_str().unwrap_or_default().to_string(),
            meth: v["meth"].as_str().unwrap_or_default().to_string(),
            nargs: v["nargs"].as_u64().unwrap_or_default() as usize,
        })
        .collect())
}

/// Every `VALUE (VALUE, ...)` entry whose name maps onto a class. Nothing
/// else can be a forward: a `char *` or a `long` in the signature means the
/// function does something a Ruby method call cannot express.
///
/// An entry ALREADY forwarded has status `zeo`, because `forward.rs` is what
/// implements it -- so filtering on `stub` alone would drop every existing row
/// on the next reverify. The rule is the SIGNATURE, and the status only
/// decides whether some other file got there first.
fn candidates() -> Result<Vec<Fwd>, Error> {
    let forwarded: Vec<String> = read_forward_rs()?.into_iter().map(|r| r.name).collect();
    let sig_re = Regex::new(r"^VALUE \(VALUE(, VALUE)*\)$").expect("a valid pattern");
    let name_re = Regex::new(r"^rb_([a-z0-9]+)_(.+)$").expect("a valid pattern");
    let mut out = Vec::new();
    // Scanned fresh rather than read back from `api.rs`: this runs only under
    // `--reverify`, which needs clang and the oracle anyway.
    for d in scan_api()? {
        if d.kind != "fn" || !(d.status == "stub" || forwarded.contains(&d.name)) {
            continue;
        }
        if !sig_re.is_match(&d.sig) || DENY.contains(&d.name.as_str()) {
            continue;
        }
        let Some(caps) = name_re.captures(&d.name) else {
            continue;
        };
        let Some((_, klass)) = PREFIX.iter().find(|(p, _)| *p == &caps[1]) else {
            continue;
        };
        let rest = &caps[2];
        let meth = match OPS.iter().find(|(k, _)| *k == rest) {
            Some((_, m)) => (*m).to_string(),
            None => match rest.strip_suffix("_p") {
                Some(base) => format!("{base}?"),
                None => match rest.strip_suffix("_bang") {
                    Some(base) => format!("{base}!"),
                    None => rest.to_string(),
                },
            },
        };
        out.push(Fwd {
            name: d.name.clone(),
            klass: (*klass).to_string(),
            meth,
            nargs: d.sig.matches("VALUE").count() - 2,
        });
    }
    Ok(out)
}

fn render_forward_rs(rows: &[Fwd]) -> String {
    let table: String = rows
        .iter()
        .map(|r| format!("    ({:?}, {:?}, {:?}, {}),\n", r.name, r.klass, r.meth, r.nargs))
        .collect();
    let entries: String = rows
        .iter()
        .map(|r| {
            let args: Vec<String> = (0..r.nargs).map(|i| format!("a{i}: Value")).collect();
            let passed: Vec<String> = (0..r.nargs).map(|i| format!("a{i}")).collect();
            let sep = if r.nargs == 0 { "" } else { ", " };
            format!(
                "/// `{}#{}`\nfn {}(recv: Value{sep}{}) -> Value {{\n    forward(recv, {:?}, &[{}])\n}}\n\n",
                r.klass,
                r.meth,
                r.name,
                args.join(", "),
                r.meth,
                passed.join(", ")
            )
        })
        .collect();
    format!(
        r##"//! `rb_*` entries that ARE a Ruby method call.
//!
//! Generated by `cargo xtask cext forward`. Do not edit.
//!
//! Most of MRI's object C API is a C name for a method Ruby already has:
//! `rb_str_length` IS `String#length`. zeo answers those by CALLING the
//! method, through the same `dispatch::send_value` a Ruby program uses -- so
//! each one sees the same MRO, the same refinements, the same
//! `method_missing` and the same visibility rules, and cannot drift from the
//! method it stands for.
//!
//! Reimplementing {n} of these in Rust would be {n} more places for
//! `String#length` to be subtly wrong.

//! # The record
//!
//! Each `FORWARDED` row is a CLAIM: MRI's `rb_x` is exactly `Class#method`,
//! so zeo can answer it by calling that method rather than reimplementing it.
//! Every row was verified against the oracle -- the class HAS that method, and
//! its arity accepts this many arguments -- which rules out a name that merely
//! looks right (`rb_ary_entry` is not `Array#entry`). It does NOT rule out a
//! name that maps onto a real method with different SEMANTICS, so `DENY` in
//! `crates/xtask/src/commands/cext.rs` names the seven that do.
//!
//! The table lives HERE, beside the bodies that stand on it, so the record
//! and the code cannot drift apart. `cext forward` reads it back to
//! regenerate the bodies; `--reverify` re-asks the oracle.

use super::convert::{{to_value, value_of}};
use super::value::Value;
use crate::{{RubyValue, Signal, Symbol}};

/// `(symbol, class, method, arity)`, sorted by symbol.
///
/// `cfg(test)`: the record is read by `cext::tests` and by
/// `cargo xtask cext forward`, never by the runtime -- the bodies below are
/// what runs.
#[cfg(test)]
pub(crate) const FORWARDED: &[(&str, &str, &str, u8)] = &[
{table}];

fn forward(recv: Value, meth: &str, args: &[Value]) -> Result<Value, Signal> {{
    // SAFETY: every argument is an extension's own live `VALUE`.
    let recv = unsafe {{ value_of(recv) }};
    let args: Vec<RubyValue> = args.iter().map(|a| unsafe {{ value_of(*a) }}).collect();
    let out = crate::dispatch::send_value(&recv, Symbol::intern(meth), &args, None)?;
    to_value(&out)
}}

crate::cext_fn! {{
{entries}
}}
"##,
        entries = entries.trim_end(),
        n = rows.len()
    )
}

// ------------------------------------------------------------------- helpers

fn files_with_extension(dir: &Path, ext: &str) -> Result<Vec<PathBuf>, Error> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?
            .path();
        if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| Error::new(format!("reading {}: {e}", path.display())))
}
