//! DWARF for compiled Ruby: `.debug_line`, plus the one-DIE
//! `.debug_info`/`.debug_abbrev` a symbolizer needs in order to find it.
//!
//! Off unless the compile asks for it (`-g`, or `ZEO_DEBUGINFO=1`). What
//! it buys is that lldb, `perf` and Instruments render a compiled Ruby
//! frame as `file.rb:line` rather than a bare address. Zeo's OWN
//! backtraces never read it -- those are built from the runtime's frame
//! stack, which is why the compiler went this far without any.
//!
//! The rows come from the `SourceLoc` the emitter stamps at each
//! statement, the same boundary `zeo_rt_set_line` marks, so the debugger's
//! answer and `caller`'s cannot drift apart.
//!
//! Addresses are RELOCATIONS, never numbers. A function's final address
//! belongs to the linker, so every row writes zero and records a
//! relocation against that function's symbol; the same goes for the
//! section-to-section offsets DWARF is full of (`DW_AT_stmt_list` and
//! friends).

use cranelift_codegen::CompiledCode;
use cranelift_module::FuncId;
use cranelift_object::ObjectProduct;
use gimli::write::{
    Address, AttributeValue, DwarfUnit, EndianVec, LineProgram, LineString, Sections, Writer,
};
use gimli::{Encoding, Format, LineEncoding, RunTimeEndian, SectionId};
use object::write::{Relocation, SectionId as ObjSectionId, StandardSegment, SymbolId};
use object::{RelocationEncoding, RelocationFlags, RelocationKind, SectionKind};

use std::collections::HashMap;

/// DWARF's own word for "no line information here". `SourceLoc::default()`
/// is `u32::MAX`, so a real row can never take that code.
const NO_SOURCE: u32 = u32::MAX;

/// One compiled function's line rows, in code order.
struct FnLines {
    func: FuncId,
    /// The function's frame label, which is what a `DW_TAG_subprogram`
    /// name reads best as (`Foo#bar`, `<main>`).
    name: String,
    len: u32,
    /// `(offset from the function's start, index into `DebugInfo::rows`)`.
    rows: Vec<(u32, u32)>,
}

/// The line table under construction, and the interning that lets a
/// `SourceLoc` -- a bare `u32` -- carry a whole `(file, line)` pair.
#[derive(Default)]
pub(crate) struct DebugInfo {
    files: Vec<String>,
    file_ids: HashMap<String, u32>,
    rows: Vec<(u32, u32)>,
    row_ids: HashMap<(u32, u32), u32>,
    funcs: Vec<FnLines>,
}

impl DebugInfo {
    /// The `SourceLoc` standing for `file:line`.
    pub(crate) fn srcloc(&mut self, file: &str, line: u32) -> cranelift_codegen::ir::SourceLoc {
        let file_idx = match self.file_ids.get(file) {
            Some(i) => *i,
            None => {
                let i = u32::try_from(self.files.len()).expect("a program has < 4G files");
                self.files.push(file.to_string());
                self.file_ids.insert(file.to_string(), i);
                i
            }
        };
        let key = (file_idx, line);
        let code = match self.row_ids.get(&key) {
            Some(c) => *c,
            None => {
                let c = u32::try_from(self.rows.len()).expect("a program has < 4G statements");
                assert!(c != NO_SOURCE, "the line table filled the SourceLoc space");
                self.rows.push(key);
                self.row_ids.insert(key, c);
                c
            }
        };
        cranelift_codegen::ir::SourceLoc::new(code)
    }

    /// Take one compiled function's source map. Called right after
    /// `define_function`, while its `CompiledCode` is still in the
    /// context.
    pub(crate) fn record(&mut self, func: FuncId, name: &str, code: &CompiledCode) {
        let rows: Vec<(u32, u32)> = code
            .buffer
            .get_srclocs_sorted()
            .iter()
            .filter(|l| !l.loc.is_default())
            .map(|l| (l.start, l.loc.bits()))
            .collect();
        if rows.is_empty() {
            return;
        }
        self.funcs.push(FnLines {
            func,
            name: name.to_string(),
            len: code.buffer.total_size(),
            rows,
        });
    }

    /// Build the DWARF sections and add them to the object.
    pub(crate) fn emit(&self, product: &mut ObjectProduct) -> Result<(), String> {
        if self.funcs.is_empty() {
            return Ok(());
        }
        let encoding = Encoding {
            format: Format::Dwarf32,
            // 4 rather than 5: a v5 line program carries its file table in
            // `.debug_line_str`, which the Mach-O debug map does not
            // follow. v4 keeps every name in the program itself.
            version: 4,
            address_size: 8,
        };
        let mut dwarf = DwarfUnit::new(encoding);
        // `LineProgram::new` PANICS on an empty string, so neither of
        // these may fall back to one.
        let comp_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string());
        let unit_name = match self.files.first() {
            Some(f) => f.clone(),
            None => return Ok(()),
        };

        let mut program = LineProgram::new(
            encoding,
            LineEncoding::default(),
            LineString::String(comp_dir.clone().into_bytes()),
            None,
            LineString::String(unit_name.clone().into_bytes()),
            None,
        );
        let dir = program.default_directory();
        let file_ids: Vec<_> = self
            .files
            .iter()
            .map(|f| program.add_file(LineString::String(f.clone().into_bytes()), dir, None))
            .collect();

        // `symbol` in a `gimli` address means whatever the writer says it
        // means; here it indexes this table, so nothing has to know how
        // `object` numbers its symbols until the relocation is applied.
        let mut symbols: Vec<SymbolId> = Vec::with_capacity(self.funcs.len());

        for f in &self.funcs {
            let symbol = symbols.len();
            symbols.push(product.function_symbol(f.func));
            program.begin_sequence(Some(Address::Symbol { symbol, addend: 0 }));
            for (offset, code) in &f.rows {
                let (file, line) = self.rows[*code as usize];
                let row = program.row();
                row.address_offset = u64::from(*offset);
                row.file = file_ids[file as usize];
                row.line = u64::from(line);
                program.generate_row();
            }
            program.end_sequence(u64::from(f.len));
        }
        dwarf.unit.line_program = program;

        let root = dwarf.unit.root();
        {
            let root = dwarf.unit.get_mut(root);
            root.set(
                gimli::DW_AT_producer,
                AttributeValue::String(format!("zeo {}", env!("CARGO_PKG_VERSION")).into_bytes()),
            );
            // DWARF 5's DW_LANG_Ruby. gimli has no constant for it.
            root.set(
                gimli::DW_AT_language,
                AttributeValue::Language(gimli::DwLang(0x0040)),
            );
            root.set(
                gimli::DW_AT_name,
                AttributeValue::String(unit_name.into_bytes()),
            );
            root.set(
                gimli::DW_AT_comp_dir,
                AttributeValue::String(comp_dir.into_bytes()),
            );
            // A unit with no address of its own is one lldb declines to
            // bind to the Mach-O debug map: it reads the map, finds no
            // range to hang the functions off, and quietly renders the
            // program with symbol names and no lines. `dsymutil` is more
            // forgiving, which is what made the omission hard to see.
            root.set(
                gimli::DW_AT_low_pc,
                AttributeValue::Address(Address::Constant(0)),
            );
        }
        // One subprogram per function: without it a symbolizer has an
        // address range and no name to hang it on.
        for (i, f) in self.funcs.iter().enumerate() {
            let die = dwarf.unit.add(root, gimli::DW_TAG_subprogram);
            let die = dwarf.unit.get_mut(die);
            die.set(
                gimli::DW_AT_name,
                AttributeValue::String(f.name.clone().into_bytes()),
            );
            die.set(
                gimli::DW_AT_low_pc,
                AttributeValue::Address(Address::Symbol {
                    symbol: i,
                    addend: 0,
                }),
            );
            die.set(
                gimli::DW_AT_high_pc,
                AttributeValue::Udata(u64::from(f.len)),
            );
        }

        // `Emitter::new` refuses a big-endian target outright.
        let mut sections = Sections::new(Relocate::new(RunTimeEndian::Little));
        dwarf
            .write(&mut sections)
            .map_err(|e| format!("writing DWARF: {e}"))?;

        // Two passes: every section must exist before a relocation can
        // name one.
        let mut ids: HashMap<SectionId, ObjSectionId> = HashMap::new();
        sections
            .for_each(|id, w| -> Result<(), String> {
                if w.writer.len() == 0 {
                    return Ok(());
                }
                let segment = product.object.segment_name(StandardSegment::Debug).to_vec();
                let obj_id = product.object.add_section(
                    segment,
                    section_name(&product.object, id),
                    SectionKind::Debug,
                );
                product
                    .object
                    .section_mut(obj_id)
                    .set_data(w.writer.slice().to_vec(), 1);
                ids.insert(id, obj_id);
                Ok(())
            })
            .map_err(|e: String| e)?;
        sections.for_each(|id, w| -> Result<(), String> {
            let Some(&obj_id) = ids.get(&id) else {
                return Ok(());
            };
            for r in &w.relocs {
                let (symbol, addend) = match r.target {
                    Target::Function(i) => section_relative(product, symbols[i], r.addend),
                    Target::Section(sec) => {
                        let Some(&target) = ids.get(&sec) else {
                            continue;
                        };
                        (product.object.section_symbol(target), r.addend)
                    }
                };
                product
                    .object
                    .add_relocation(
                        obj_id,
                        Relocation {
                            offset: u64::from(r.offset),
                            symbol,
                            addend,
                            flags: RelocationFlags::Generic {
                                kind: RelocationKind::Absolute,
                                encoding: RelocationEncoding::Generic,
                                size: r.size * 8,
                            },
                        },
                    )
                    .map_err(|e| format!("relocating a debug section: {e}"))?;
            }
            Ok(())
        })
    }
}

/// A debug reference to `func`, expressed against the SECTION that
/// defines it rather than against the function symbol itself.
///
/// Mach-O's reason: a `-g` program's DWARF never reaches the binary. The
/// linker leaves a debug MAP behind instead, and `dsymutil` reads the
/// DWARF back out of the object file, applying the object's own
/// relocations as it goes -- but only the ones it recognises, and a local
/// function symbol is not one of them (LLVM never emits that shape; it
/// always points a debug address at a section plus an offset). An
/// unrecognised relocation is not an error there, it is silence: every
/// row keeps the raw offset it was written with, lands in whichever
/// function happens to occupy that offset, and the line table comes out
/// confidently wrong.
///
/// A section reference costs nothing on ELF, where the linker applies
/// either shape.
fn section_relative(product: &mut ObjectProduct, func: SymbolId, addend: i64) -> (SymbolId, i64) {
    let sym = product.object.symbol(func);
    let (section, offset) = match sym.section {
        object::write::SymbolSection::Section(id) => (id, sym.value),
        _ => return (func, addend),
    };
    let offset = i64::try_from(offset).unwrap_or(0);
    (product.object.section_symbol(section), addend + offset)
}

/// A debug section's name in `obj`'s format. Mach-O spells them
/// `__debug_line` inside the `__DWARF` segment, ELF `.debug_line`.
fn section_name(obj: &object::write::Object<'_>, id: SectionId) -> Vec<u8> {
    if obj.format() == object::BinaryFormat::MachO {
        format!("__{}", id.name().trim_start_matches('.')).into_bytes()
    } else {
        id.name().as_bytes().to_vec()
    }
}

/// What a recorded relocation points at.
#[derive(Clone, Copy)]
enum Target {
    /// An index into the writer's caller-owned symbol table.
    Function(usize),
    /// Another debug section, by DWARF's own id.
    Section(SectionId),
}

#[derive(Clone, Copy)]
struct Reloc {
    offset: u32,
    size: u8,
    addend: i64,
    target: Target,
}

/// A `gimli` writer that turns every address and cross-section offset into
/// an object relocation instead of a number.
#[derive(Clone)]
struct Relocate {
    writer: EndianVec<RunTimeEndian>,
    relocs: Vec<Reloc>,
}

impl Relocate {
    fn new(endian: RunTimeEndian) -> Self {
        Relocate {
            writer: EndianVec::new(endian),
            relocs: Vec::new(),
        }
    }
}

impl Writer for Relocate {
    type Endian = RunTimeEndian;

    fn endian(&self) -> Self::Endian {
        self.writer.endian()
    }

    fn len(&self) -> usize {
        self.writer.len()
    }

    fn write(&mut self, bytes: &[u8]) -> gimli::write::Result<()> {
        self.writer.write(bytes)
    }

    fn write_at(&mut self, offset: usize, bytes: &[u8]) -> gimli::write::Result<()> {
        self.writer.write_at(offset, bytes)
    }

    fn write_address(&mut self, address: Address, size: u8) -> gimli::write::Result<()> {
        match address {
            Address::Constant(v) => self.write_udata(v, size),
            Address::Symbol { symbol, addend } => {
                let offset = self.len() as u32;
                self.relocs.push(Reloc {
                    offset,
                    size,
                    addend,
                    target: Target::Function(symbol),
                });
                self.write_udata(0, size)
            }
        }
    }

    fn write_offset(
        &mut self,
        val: usize,
        section: SectionId,
        size: u8,
    ) -> gimli::write::Result<()> {
        let offset = self.len() as u32;
        self.relocs.push(Reloc {
            offset,
            size,
            addend: val as i64,
            target: Target::Section(section),
        });
        self.write_udata(0, size)
    }

    fn write_offset_at(
        &mut self,
        offset: usize,
        val: usize,
        section: SectionId,
        size: u8,
    ) -> gimli::write::Result<()> {
        self.relocs.push(Reloc {
            offset: offset as u32,
            size,
            addend: val as i64,
            target: Target::Section(section),
        });
        self.write_udata_at(offset, 0, size)
    }
}
