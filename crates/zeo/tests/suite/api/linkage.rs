//! The link ledger: what the AOT link must KEEP, and what it must DROP.
//!
//! They are opposite halves of the same link line (`backend/link.rs`), and
//! a regression in either is silent:
//!
//! - **Keep.** A program names the builtin class tables it can reach
//!   (`zeo_ctable_<ID>`), and the link uses `-force_load` / `--whole-archive`
//!   so an archive member holding one is a candidate. Lose a table and the
//!   program still links and still runs -- it just answers `NoMethodError`
//!   for whatever class went missing, which reads as a dispatch bug rather
//!   than a link bug.
//! - **Drop.** `libzeo.a` carries the COMPILER as well as the runtime
//!   (one staticlib), and `-dead_strip` / `--gc-sections`
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
    // The link needs the archive, which a test run does not build.
    crate::paths::runtime_archive().unwrap_or_else(|e| panic!("{e}"));
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
    crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}"))
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Asserts `tables` is a NARROWED set rather than the whole always-on one.
///
/// Set equality against an eval-free program is the wrong test: two narrowed
/// programs reach different classes on purpose, and a spliced unit's own body
/// legitimately adds its own. What must hold is that the program did not fall
/// back to "everything" -- which is what carrying the compiler looks like
/// through the symbol table.
fn assert_narrowed(
    tables: &std::collections::BTreeSet<String>,
    eval_free: &std::collections::BTreeSet<String>,
    what: &str,
) {
    let known = zeo::builtin_surface::CLASS_TABLE_SYMBOLS.len();
    assert!(
        tables.len() < known / 2,
        "{what} named {} of {known} class tables -- it took the whole \
         always-on set, which is what an eval-capable program does",
        tables.len()
    );
    // The classes an eval-free program cannot reach either. Naming any of
    // them means the narrowing stopped applying, not that this program
    // reaches more.
    for gone in [
        "zeo_ctable_MARSHAL_MODULE",
        "zeo_ctable_RACTOR_CLASS",
        "zeo_ctable_TRACEPOINT_CLASS",
    ] {
        assert!(
            !tables.contains(gone),
            "{what} cannot reach {gone}, and an eval-free program does not \
             name it either (it named {} tables against {})",
            tables.len(),
            eval_free.len()
        );
    }
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

/// An always-on builtin's table is carried only when the program can REACH
/// the class.
///
/// `analyze::class_reach` answers which of the always-on classes (`Ractor`,
/// `Marshal`, `TracePoint`, `Pathname`, ...) a value can arrive as, so
/// `puts :ok` ships none of them. `cargo xtask size --check` is the gate on
/// what that saves.
///
/// Asserted through the exported `zeo_ctable_*` symbols rather than bytes,
/// because that names WHICH class went.
#[test]
fn an_unreachable_always_on_class_leaves_its_table_behind() {
    let plain = link_program("puts :ok\n");
    let tables = ctable_symbols(&plain);
    let _ = std::fs::remove_file(&plain);

    for gone in [
        "zeo_ctable_MARSHAL_MODULE",
        "zeo_ctable_RACTOR_CLASS",
        "zeo_ctable_TRACEPOINT_CLASS",
        "zeo_ctable_PATHNAME_CLASS",
        "zeo_ctable_TIME_CLASS",
        "zeo_ctable_REGEXP_CLASS",
        "zeo_ctable_MATH_CLASS",
        "zeo_ctable_STRUCT_CLASS",
    ] {
        assert!(
            !tables.contains(gone),
            "`puts :ok` cannot reach {gone} -- carrying it is dead weight"
        );
    }

    // Naming one is the plainest channel there is, and it brings the class's
    // whole chain with it.
    let timed = link_program("puts Time.at(0).year\n");
    let named = ctable_symbols(&timed);
    let _ = std::fs::remove_file(&timed);
    assert!(named.contains("zeo_ctable_TIME_CLASS"));
    assert!(named.contains("zeo_ctable_COMPARABLE_CLASS"));

    // A class a called row RETURNS, with the program naming it nowhere.
    let framed = link_program("p caller_locations(1, 1)\n");
    let frames = ctable_symbols(&framed);
    let _ = std::fs::remove_file(&framed);
    assert!(
        frames.contains("zeo_ctable_BACKTRACE_LOCATION_CLASS"),
        "`caller_locations` hands back a class no scan of constants can see"
    );

    // And a give-everything hatch takes the lot back.
    let reflective = link_program("p Marshal.load(Marshal.dump([1]))\n");
    let all = ctable_symbols(&reflective);
    let _ = std::fs::remove_file(&reflective);
    assert!(
        all.len() > tables.len() + 40,
        "`Marshal.load` rebuilds an arbitrary graph, so it must keep every \
         always-on table: it named {} against `puts :ok`'s {}",
        all.len(),
        tables.len()
    );
}

/// The dead-strip half: a program that cannot `eval` does not carry the
/// compiler. `libzeo.a` holds compiler and runtime alike (one staticlib)
/// and `-force_load`/`--whole-archive` pulls in every member, so
/// `-dead_strip`/`--gc-sections` is the only thing standing
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
/// every always-on table with it. A receiverless call resolves the way dispatch resolves
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
    assert_narrowed(&tables, &plain_tables, "a program whose `load` is its own");
}

/// A `require` inside a method body, of a feature this compile emitted as a
/// UNIT, dead-strips the compiler too.
///
/// The call stays a runtime `Kernel#require` on purpose -- CRuby loads such a
/// file when the method runs -- and `dynamic_require` asks
/// `features::load_feature` before the on-disk tier that needs a compiler. So
/// the unit answers it, and the compiler is not owed. A predicate that
/// matched the call's NAME alone would link Cranelift and all 170 class
/// tables for this program.
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
    assert_narrowed(
        &tables,
        &plain_tables,
        "a deferred require of a compiled-in unit",
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
/// with them goes the prism library they root, out of a `puts 1`
/// binary, measured with `cargo xtask size`.
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
    // `RubyVM` the CONSTANT is still there, because ruby's is -- a class's
    // registration comes from the runtime's own builtin list, so dropping a
    // table takes its rows and never its name.
    let probe = link_program("p Object.const_defined?(:RubyVM)\n");
    let out = std::process::Command::new(&probe)
        .output()
        .expect("running the probe");
    let _ = std::fs::remove_file(&probe);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "true\n",
        "a program that carries no RubyVM table must still answer for the \
         constant -- ruby defines it in every program"
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

// ---- `--link`: what the caller adds to the line. --------------------------
//
// Both facts below are silent on failure in the other direction: a dropped
// `-sectcreate` leaves a binary that runs and has no payload, and a carried
// object whose symbol is not exported links fine and raises at the FFI call.

/// Compile `source` with `link_args` on the line, link it, run it, and hand
/// back what it printed. The library-API path: `CompileOptions::link_args`
/// rides through `ObjectOutput` to the link.
fn link_and_run(source: &str, link_args: &[String]) -> std::process::Output {
    crate::paths::runtime_archive().unwrap_or_else(|e| panic!("{e}"));
    let opts = zeo::CompileOptions {
        link_args: link_args.to_vec(),
        ..Default::default()
    };
    let compiled = zeo::compile_to_object_with(source, &opts, false)
        .unwrap_or_else(|e| panic!("compile_to_object_with failed: {e}"));
    let bin = std::env::temp_dir().join(format!(
        "zeo-linkarg-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    zeo::backend::build_artifact(&zeo::backend::CompiledProgram::Aot(&compiled), &bin)
        .unwrap_or_else(|e| panic!("linking the test binary failed: {e}"));
    let out = std::process::Command::new(&bin)
        .output()
        .unwrap_or_else(|e| panic!("running the linked binary: {e}"));
    let _ = std::fs::remove_file(&bin);
    out
}

/// A `-sectcreate` payload survives the link, and the program reads it back
/// through libSystem's own section API -- the shape an application that
/// carries its assets as Mach-O sections takes.
///
/// macOS only: `-sectcreate` and `getsectiondata` are ld64's and dyld's. The
/// Linux analogue of "a link argument reaches the binary" is the carried
/// object below, which runs on both.
#[cfg(target_os = "macos")]
#[test]
fn a_link_arg_carries_a_section_the_program_reads_back() {
    let payload = std::env::temp_dir().join(format!("zeo-sect-{}.bin", std::process::id()));
    std::fs::write(&payload, b"sow probe payload\n").expect("writing the payload");
    let out = link_and_run(
        r#"
        require "ffi"
        module Probe
          extend FFI::Library
          ffi_lib FFI::CURRENT_PROCESS
          attach_function :_dyld_get_image_header, [:uint32], :pointer
          attach_function :getsectiondata, [:pointer, :string, :string, :pointer], :pointer
        end
        size = FFI::MemoryPointer.new(:ulong)
        data = Probe.getsectiondata(Probe._dyld_get_image_header(0), "__SOW", "__probe", size)
        print data.read_bytes(size.read_ulong)
        "#,
        &[format!(
            "-Wl,-sectcreate,__SOW,__probe,{}",
            payload.display()
        )],
    );
    let _ = std::fs::remove_file(&payload);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "sow probe payload\n");
}

/// `int zeo_probe_add(int, int)` compiled to an object in `dir`, or `None`
/// when the machine has no C compiler (the test then has nothing to link).
fn probe_object(dir: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).expect("the scratch dir");
    let c = dir.join("probe.c");
    let o = dir.join("probe.o");
    std::fs::write(&c, "int zeo_probe_add(int a, int b) { return a + b; }\n").expect("probe.c");
    let out = std::process::Command::new("cc")
        .args(["-c", "-o"])
        .arg(&o)
        .arg(&c)
        .output()
        .ok()?;
    assert!(
        out.status.success(),
        "cc -c failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(o)
}

/// The linker's spelling for "export this one symbol": ld64's and GNU ld's.
fn export_flag(symbol: &str) -> String {
    match cfg!(target_os = "macos") {
        true => format!("-Wl,-exported_symbol,_{symbol}"),
        false => format!("-Wl,--export-dynamic-symbol={symbol}"),
    }
}

const ADD_RB: &str = r#"
require "ffi"
module Probe
  extend FFI::Library
  ffi_lib FFI::CURRENT_PROCESS
  attach_function :zeo_probe_add, [:int, :int], :int
end
puts Probe.zeo_probe_add(2, 3)
"#;

/// `zeo build ... -o bin` with the given `--link` arguments, run once.
fn build_and_run(
    dir: &Path,
    link_flags: &[String],
    env: &[(&str, String)],
) -> std::process::Output {
    let rb = dir.join("add.rb");
    std::fs::write(&rb, ADD_RB).expect("add.rb");
    let bin = dir.join("add");
    let _ = std::fs::remove_file(&bin);
    let mut cmd = std::process::Command::new(zeo_cli());
    cmd.env_remove("RUBYOPT").env_remove("RUBYLIB");
    cmd.arg("build")
        .arg(&rb)
        .arg("-o")
        .arg(&bin)
        .args(link_flags);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawning zeo");
    assert!(
        out.status.success(),
        "zeo build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::process::Command::new(&bin)
        .output()
        .unwrap_or_else(|e| panic!("running {}: {e}", bin.display()))
}

/// A carried C object is linked in through the CLI (both `--link <arg>` and
/// `--link=<arg>` spellings) and reached from Ruby through
/// `FFI::CURRENT_PROCESS` -- once its symbol is exported. Without the export
/// the same link succeeds and the call cannot find the symbol, which is the
/// rule `docs/explanation/backend.md` writes down.
#[test]
fn a_carried_object_is_reached_through_ffi_when_its_symbol_is_exported() {
    let dir = std::env::temp_dir().join(format!("zeo-linkobj-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let Some(obj) = probe_object(&dir) else {
        eprintln!("skipping: this machine has no C compiler");
        return;
    };
    let exported = build_and_run(
        &dir,
        &[
            "--link".to_string(),
            obj.display().to_string(),
            format!("--link={}", export_flag("zeo_probe_add")),
        ],
        &[],
    );
    assert!(
        exported.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&exported.stdout), "5\n");

    let hidden = build_and_run(
        &dir,
        &["--link".to_string(), obj.display().to_string()],
        &[],
    );
    assert!(
        !hidden.status.success(),
        "an unexported symbol must not be reachable through dlsym; stdout: {}",
        String::from_utf8_lossy(&hidden.stdout)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `ZEO_LINK_ARGS` is the same list, whitespace-split.
#[test]
fn zeo_link_args_is_the_env_spelling_of_link() {
    let dir = std::env::temp_dir().join(format!("zeo-linkenv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let Some(obj) = probe_object(&dir) else {
        eprintln!("skipping: this machine has no C compiler");
        return;
    };
    let out = build_and_run(
        &dir,
        &[],
        &[(
            "ZEO_LINK_ARGS",
            format!("{} {}", obj.display(), export_flag("zeo_probe_add")),
        )],
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "5\n",
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
