//! The link ledger: what the AOT link must KEEP, and what it must DROP.
//!
//! Both facts were asserted only implicitly until now -- the M0/M0.5 gates
//! recorded them as owed. They are opposite halves of the same link line
//! (`backend/link.rs`), and a regression in either is silent:
//!
//! - **Keep.** A program names the builtin class tables it can reach
//!   (`zeo_ctable_<ID>`), and the link uses `-force_load` / `--whole-archive`
//!   so an archive member holding one is a candidate. Lose a table and the
//!   program still links and still runs -- it just answers `NoMethodError`
//!   for whatever class went missing, which reads as a dispatch bug rather
//!   than a link bug.
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

/// The `zeo_ctable_*` symbols `binary` resolved -- one per builtin class
/// table the program named, with the leading Mach-O underscore trimmed.
fn ctable_symbols(binary: &Path) -> std::collections::BTreeSet<String> {
    let bytes = read(binary);
    let file = object::File::parse(&*bytes)
        .unwrap_or_else(|e| panic!("parsing {}: {e}", binary.display()));
    use object::ObjectSymbol;
    file.symbols()
        .filter_map(|sym| sym.name().ok())
        .map(|n| n.strip_prefix('_').unwrap_or(n).to_string())
        .filter(|n| n.starts_with("zeo_ctable_"))
        .collect()
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

/// A linked program carries exactly the builtin class tables it NAMED --
/// every one it can reach, and none it cannot.
///
/// This is what a program's table list is FOR. A table roots its class's
/// whole method surface, so `Regexp`'s drags in two regex engines and
/// `RubyVM::AST`'s drags in prism; naming one a program cannot reach costs
/// that for nothing, and failing to name one it CAN reach loses every method
/// and constant that class has, silently and only at run time.
///
/// Both directions are asserted, because each fails invisibly on its own. A
/// symbol the archive defines but the program does not name is simply absent
/// at run time; a symbol named for a class the program cannot reach just
/// makes the binary bigger.
///
/// Symbols answer this where bytes cannot: `zeo_ctable_*` are exported
/// (`#[unsafe(export_name)]` in zeo-macros), so they survive the link's `-x`,
/// which discards only the LOCAL symbol table.
#[test]
fn a_linked_program_keeps_the_tables_it_names() {
    let bin = link_program("puts :ok\n");
    let linked = ctable_symbols(&bin);
    let _ = std::fs::remove_file(&bin);

    let known: std::collections::BTreeSet<&str> = zeo::builtin_surface::CLASS_TABLE_SYMBOLS
        .iter()
        .map(|(_, s)| *s)
        .collect();
    let invented: Vec<&String> = linked
        .iter()
        .filter(|n| !known.contains(n.as_str()))
        .collect();
    assert!(
        invented.is_empty(),
        "the program names table symbols the compiler does not know: {invented:?}"
    );

    // A class any program can reach. `puts :ok` never writes `String`, but a
    // value of that kind arrives without being named, so its table stays.
    for core in [
        "zeo_ctable_STRING_CLASS",
        "zeo_ctable_ARRAY_CLASS",
        "zeo_ctable_INTEGER_CLASS",
        "zeo_ctable_KERNEL_CLASS",
    ] {
        assert!(
            linked.contains(core),
            "a program without {core} loses that class's every method: {:?}",
            linked.len()
        );
    }

    // A require-gated class this program never asks for. Each of these is a
    // whole vendored C library -- dropping them is the point.
    for gated in [
        "zeo_ctable_OPENSSL_DIGEST_SHA256_CLASS",
        "zeo_ctable_ZLIB_MODULE",
        "zeo_ctable_PSYCH_MODULE",
        "zeo_ctable_SOCKET_CLASS",
    ] {
        assert!(
            !linked.contains(gated),
            "`puts :ok` cannot reach {gated}, so naming it links that class's \
             whole surface for nothing"
        );
    }
    assert!(
        linked.len() < known.len(),
        "a program that requires nothing named ALL {} tables -- the reachability \
         gate in `needed_class_tables` is not narrowing at all",
        known.len()
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

/// A program whose only `load`/`eval`/`require` calls are ITS OWN methods
/// dead-strips the compiler too.
///
/// The predicate that decides matched a call's NAME and nothing else, so
/// `def load(x); load(1); end` shipped Cranelift and all 170 class tables --
/// 14 MB, measured. A receiverless call resolves the way dispatch resolves
/// it, and Kernel's row is unreachable from a class that defines its own.
///
/// Measured against the eval-free program rather than an absolute size: what
/// is asserted is that the two carry the same thing.
#[test]
fn a_program_with_its_own_load_dead_strips_the_compiler() {
    let plain = link_program("puts :ok\n");
    let baseline = text_bytes(&plain);
    let plain_tables = ctable_symbols(&plain);
    let _ = std::fs::remove_file(&plain);

    let bin = link_program("def load(x) = x\nputs load(1)\n");
    let program = text_bytes(&bin);
    let tables = ctable_symbols(&bin);
    let _ = std::fs::remove_file(&bin);

    let ratio = program as f64 / baseline as f64;
    assert!(
        ratio < 1.10,
        "a program whose `load` is its own has {program} bytes of text, \
         {ratio:.2}x an eval-free program's {baseline} -- it is carrying the \
         compiler. See `Compiler::runtime_eval`."
    );
    assert_eq!(
        tables, plain_tables,
        "it should name the same class tables as an eval-free program, not \
         the whole set an eval-capable one takes"
    );
}

/// A `require` inside a method body, of a feature this compile emitted as a
/// UNIT, dead-strips the compiler too.
///
/// The call stays a runtime `Kernel#require` on purpose -- CRuby loads such a
/// file when the method runs -- and `dynamic_require` asks
/// `features::load_feature` before the on-disk tier that needs a compiler. So
/// the unit answers it, and the 14 MB is not owed. It used to be: the
/// predicate matched the call's NAME, and this program linked Cranelift and
/// all 170 class tables.
///
/// The two shapes that still carry the compiler are the point of the second
/// half: a feature that resolved to NO unit, and a COMPUTED target. Both are
/// correct, and asserting them is what keeps the narrowing exact rather than
/// optimistic.
#[test]
fn a_deferred_require_of_a_compiled_unit_dead_strips_the_compiler() {
    let plain = link_program("puts :ok\n");
    let baseline = text_bytes(&plain);
    let plain_tables = ctable_symbols(&plain);
    let _ = std::fs::remove_file(&plain);

    let bin = link_program("def lazy = require \"prettyprint\"\nputs lazy\n");
    let program = text_bytes(&bin);
    let tables = ctable_symbols(&bin);
    let _ = std::fs::remove_file(&bin);

    let ratio = program as f64 / baseline as f64;
    assert!(
        ratio < 1.20,
        "a deferred require of a compiled-in unit has {program} bytes of text, \
         {ratio:.2}x an eval-free program's {baseline} -- it is carrying the \
         compiler. See `analyze::compiled_in_requires`."
    );
    assert_eq!(
        tables, plain_tables,
        "it should name the same class tables as an eval-free program"
    );

    // A feature no unit answers, and a COMPUTED target: both reach the
    // on-disk tier, so both must still carry the compiler.
    for src in [
        "def lazy\n  require \"no_such_feature_anywhere\"\nrescue LoadError\n  :no\nend\nputs lazy\n",
        "def lazy(n)\n  require n\nrescue LoadError\n  :no\nend\nputs lazy(\"prettyprint\")\n",
    ] {
        let bin = link_program(src);
        let program = text_bytes(&bin);
        let _ = std::fs::remove_file(&bin);
        assert!(
            program as f64 / baseline as f64 > 1.50,
            "`{}` has {program} bytes of text against {baseline} -- it needs \
             the compiler and must not have been narrowed",
            src.lines().next().unwrap_or(src)
        );
    }
}

/// The prism-backed `RubyVM` surfaces are not always-on.
///
/// `RubyVM::AbstractSyntaxTree` and `RubyVM::InstructionSequence` PARSE at run
/// time, and no program can reach either without naming `RubyVM` -- they are
/// namespaced under it. So a program that never does carries neither, and
/// with them goes the prism library they root: 528,800 bytes of a `puts 1`
/// binary, measured with `zeo-dev size`.
///
/// `RubyVM` itself stays. ruby defines it in every program, and only
/// `RubyVM.constants` can see the five go -- which already names `RubyVM`.
#[test]
fn a_program_that_never_names_rubyvm_drops_the_parser_tables() {
    let plain = link_program("puts :ok\n");
    let plain_tables = ctable_symbols(&plain);
    let baseline = text_bytes(&plain);
    let _ = std::fs::remove_file(&plain);

    for gone in [
        "zeo_ctable_RUBYVM_AST_MODULE",
        "zeo_ctable_RUBYVM_AST_NODE_CLASS",
        "zeo_ctable_RUBYVM_AST_LOCATION_CLASS",
        "zeo_ctable_RUBYVM_ISEQ_CLASS",
        "zeo_ctable_RUBYVM_YJIT_MODULE",
    ] {
        assert!(
            !plain_tables.contains(gone),
            "`puts :ok` cannot reach {gone} without naming RubyVM"
        );
    }
    // `RubyVM` itself is always there, because ruby's is.
    assert!(
        plain_tables.contains("zeo_ctable_RUBYVM_CLASS"),
        "RubyVM is a constant in every ruby program"
    );

    // Naming it brings them all back, and the program is bigger for it.
    let bin = link_program("puts RubyVM::AbstractSyntaxTree.parse(\"1\").type\n");
    let named = ctable_symbols(&bin);
    let with_ast = text_bytes(&bin);
    let _ = std::fs::remove_file(&bin);
    assert!(
        named.contains("zeo_ctable_RUBYVM_AST_MODULE"),
        "a program that parses an AST must carry the table that does it"
    );
    assert!(
        with_ast > baseline,
        "the AST surface costs {with_ast} against {baseline} -- it should not \
         be free, and if it is, the drop above is not what made it absent"
    );
}
