//! The `-g` ledger: a compiled program's DWARF names the RUBY file and
//! the RUBY line, and a program compiled without `-g` carries none.
//!
//! Read off the emitted object rather than a debugger, because the two
//! ways a debugger reaches this differ by platform -- ELF links the DWARF
//! into the binary, Mach-O leaves a debug map pointing back at the object
//! file -- while the object is the same artifact on both.

use gimli::{EndianSlice, LittleEndian};
use object::{Object, ObjectSection};

/// Compile `source` as a real file called `name`, with or without debug
/// info. The file has to exist: the loader resolves the input path, and
/// the name it resolves to is what the line table reports.
fn object_for(source: &str, name: &str, debuginfo: bool) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("zeo-dbg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join(name);
    std::fs::write(&path, source).expect("writing the scratch source");
    let opts = zeo::CompileOptions {
        input_path: Some(path.clone()),
        ..Default::default()
    };
    let object = zeo::compile_to_object_with(source, &opts, debuginfo)
        .unwrap_or_else(|e| panic!("compiling {name}: {e}"))
        .object;
    let _ = std::fs::remove_file(&path);
    object
}

/// A named section's bytes. The name is spelled per format: Mach-O puts
/// DWARF in a `__DWARF` segment and drops the dot.
fn section(bytes: &[u8], dwarf_name: &str) -> Option<Vec<u8>> {
    let file = object::File::parse(bytes).expect("parsing the emitted object");
    let macho = format!("__{}", dwarf_name.trim_start_matches('.'));
    file.sections()
        .find(|s| s.name().is_ok_and(|n| n == dwarf_name || n == macho))
        .map(|s| s.data().expect("section data").to_vec())
}

/// Every `(file, line)` the object's line program reports.
fn line_rows(object: &[u8]) -> Vec<(String, u64)> {
    let data = section(object, ".debug_line").expect("a -g object has a line program");
    let debug_line = gimli::DebugLine::new(&data, LittleEndian);
    let program = debug_line
        .program(gimli::DebugLineOffset(0), 8, None, None)
        .expect("reading the line program");
    let mut out = Vec::new();
    let mut rows = program.rows();
    while let Some((header, row)) = rows.next_row().expect("stepping the line program") {
        if row.end_sequence() {
            continue;
        }
        let file = row
            .file(header)
            .and_then(|f| {
                f.path_name()
                    .string_value(&gimli::DebugStr::from(EndianSlice::new(&[], LittleEndian)))
            })
            .map_or_else(String::new, |v| {
                String::from_utf8_lossy(v.slice()).into_owned()
            });
        out.push((file, row.line().map_or(0, std::num::NonZeroU64::get)));
    }
    out
}

const PROGRAM: &str = "def add(a, b)\n  a + b\nend\n\nx = add(2, 3)\nputs x\n";

#[test]
fn a_debug_build_maps_addresses_to_ruby_lines() {
    let object = object_for(PROGRAM, "adder.rb", true);
    let rows = line_rows(&object);
    // Every row names a Ruby file. Two kinds qualify: the program's own
    // source, and a vendored corelib segment -- CRuby's `nilclass.rb` is
    // compiled into every program and its bodies carry line rows of their
    // own, under the `<internal:>` name CRuby itself reports.
    assert!(
        rows.iter()
            .all(|(f, _)| f.ends_with("adder.rb") || f.starts_with("<internal:")),
        "every row names the Ruby source or a corelib segment, got {:?}",
        rows.iter()
            .map(|(f, _)| f)
            .collect::<std::collections::BTreeSet<_>>()
    );
    let lines: std::collections::BTreeSet<u64> = rows
        .iter()
        .filter(|(f, _)| f.ends_with("adder.rb"))
        .map(|(_, l)| *l)
        .collect();
    // 2 is the method body, 5 and 6 the top level. A body line is the
    // proof the table follows the FUNCTION and not the file: the two live
    // in separate compiled functions and separate line sequences.
    for line in [2, 5, 6] {
        assert!(
            lines.contains(&line),
            "no row for {}:{line} -- rows: {lines:?}",
            "adder.rb"
        );
    }
}

#[test]
fn an_ordinary_build_carries_no_debug_info() {
    let object = object_for(PROGRAM, "adder.rb", false);
    for name in [".debug_line", ".debug_info", ".debug_abbrev"] {
        assert!(
            section(&object, name).is_none(),
            "a build without -g emitted {name}; debug info is opt-in"
        );
    }
}
