//! The link ledger: what the AOT link must KEEP, and what it must DROP.
//!
//! Both facts were asserted only implicitly until now -- the M0/M0.5 gates
//! recorded them as owed. They are opposite halves of the same link line
//! (`backend/link.rs`), and a regression in either is silent:
//!
//! - **Keep.** linkme's `BUILTIN_TABLES` elements live in archive members
//!   nothing references by name, so the link uses `-force_load` /
//!   `--whole-archive`. Lose that and the program still links and still
//!   runs -- it just answers `NoMethodError` for whatever classes went
//!   missing, which reads as a dispatch bug, not a link bug.
//! - **Drop.** `libzeo.a` carries the COMPILER as well as the runtime
//!   (one staticlib, plan decision 9), and `-dead_strip` / `--gc-sections`
//!   is what keeps an eval-free program from shipping Cranelift. Lose that
//!   and nothing fails; the binary just doubles.
//!
//! Both are measured on a REAL linked program, against the `zeo` binary as
//! the reference for what the archive holds.

use std::path::{Path, PathBuf};

use object::{Object, ObjectSection};

/// Compile and link `source` into a throwaway binary, run it once to prove
/// it is a working program (an assert on a binary that cannot run proves
/// nothing), and hand back the path. The caller removes it.
fn link_program(source: &str) -> PathBuf {
    let opts = zeo::CompileOptions::default();
    let compiled = zeo::compile_to_object_with(source, &opts, false)
        .unwrap_or_else(|e| panic!("compile_to_object_with failed: {e}"));
    let bin = std::env::temp_dir().join(format!(
        "zeo-linkage-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    zeo::backend::build_artifact(&zeo::backend::CompiledProgram::Aot(&compiled), &bin)
        .unwrap_or_else(|e| panic!("linking the test binary failed: {e}"));
    let out = std::process::Command::new(&bin)
        .output()
        .unwrap_or_else(|e| panic!("running the linked binary: {e}"));
    assert!(
        out.status.success(),
        "the linked program must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    bin
}

/// The `zeo` CLI beside this test binary's profile dir -- built from the
/// same cargo invocation as the `libzeo.a` the program links, so it is the
/// reference for what that archive holds.
fn zeo_cli() -> PathBuf {
    let mut p = std::env::current_exe().expect("test binary path");
    p.pop(); // deps/<test-bin> -> deps
    p.pop(); // deps -> target/<profile>
    p.push("zeo");
    assert!(
        p.is_file(),
        "these tests need the zeo CLI at {} (run `cargo build -p zeo` first)",
        p.display()
    );
    p
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Total byte size of every linkme distributed-slice section in `binary`.
///
/// linkme names its section after a hash of the slice, and truncates to
/// Mach-O's 16-character limit (`__linkmeN8DwnQlp`), so the name cannot be
/// spelled portably -- every `linkme` section is summed instead. The
/// runtime declares exactly one distributed slice (`BUILTIN_TABLES`), so
/// the sum IS its size, and a dropped element shows up as a smaller sum.
fn linkme_bytes(binary: &Path) -> u64 {
    let bytes = read(binary);
    let file = object::File::parse(&*bytes)
        .unwrap_or_else(|e| panic!("parsing {}: {e}", binary.display()));
    let total: u64 = file
        .sections()
        .filter(|s| s.name().is_ok_and(|n| n.contains("linkme")))
        .map(|s| s.size())
        .sum();
    assert!(
        total > 0,
        "{} carries no linkme section at all",
        binary.display()
    );
    total
}

/// Total size of `binary`'s executable sections -- the quantity the
/// compiler's presence or absence moves.
///
/// Symbol names cannot answer this: the link discards the local symbol
/// table (`-x`), so a compiler that survived would be nameless but
/// present. Bytes of code are what is actually there.
fn text_bytes(binary: &Path) -> u64 {
    let bytes = read(binary);
    let file = object::File::parse(&*bytes)
        .unwrap_or_else(|e| panic!("parsing {}: {e}", binary.display()));
    let total: u64 = file
        .sections()
        .filter(|s| s.kind() == object::SectionKind::Text)
        .map(|s| s.size())
        .sum();
    assert!(
        total > 0,
        "{} carries no text section at all",
        binary.display()
    );
    total
}

/// The whole-archive half: a linked program's `BUILTIN_TABLES` is the same
/// size as the `zeo` binary's, element for element.
#[test]
fn a_linked_program_keeps_every_builtin_table() {
    let bin = link_program("puts :ok\n");
    let program = linkme_bytes(&bin);
    let compiler = linkme_bytes(&zeo_cli());
    let _ = std::fs::remove_file(&bin);
    assert_eq!(
        program, compiler,
        "the linked program's builtin tables ({program} bytes) differ from the \
         zeo binary's ({compiler} bytes) -- the link dropped or duplicated \
         linkme elements (see -force_load/--whole-archive in backend/link.rs)"
    );
}

/// The dead-strip half: a program that cannot `eval` does not carry the
/// compiler. `libzeo.a` holds compiler and runtime alike (one staticlib,
/// plan decision 9) and `-force_load`/`--whole-archive` pulls in every
/// member, so `-dead_strip`/`--gc-sections` is the only thing standing
/// between an eval-free program and a copy of Cranelift.
///
/// Measured against the `zeo` binary, which links the same archive and
/// DOES reference the compiler. Today a linked program's text is about
/// half of it; drop the flag and it is 1.2x of it, because the program
/// then carries every member the archive has. Anything in between fails
/// loudly rather than passing on a coincidence.
#[test]
fn an_eval_free_program_dead_strips_the_compiler() {
    let compiler = text_bytes(&zeo_cli());
    let bin = link_program("puts :ok\n");
    let program = text_bytes(&bin);
    let _ = std::fs::remove_file(&bin);

    let ratio = program as f64 / compiler as f64;
    assert!(
        ratio < 0.75,
        "an eval-free program's text is {program} bytes, {ratio:.2}x the zeo \
         binary's {compiler} -- it is carrying the compiler half of \
         libzeo.a. The link no longer dead-strips (see \
         -dead_strip/--gc-sections in backend/link.rs); measured, losing \
         that flag takes this ratio from 0.52 to 1.21."
    );
}
