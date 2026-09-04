//! The `.zeodata` sidecar: what a program's CLIF text cannot say.
//!
//! CLIF carries code. Everything registration-shaped -- the interned
//! symbols, the inline-cache counts, the read-only bytes the code indexes,
//! the method rows `zeo_rt_main` registers, the builtin class tables the
//! program keeps -- is described here and materialised by `zeo backend`
//! with `zeo-abi`'s own layouts, so a front end writing CLIF never learns
//! a struct offset. `--emit-zeodata` writes one from the Rust emitter; a
//! front end in another language writes the same JSON.
//!
//! The sidecar describes a SUBSET: top-level `def`s and their trampolines,
//! symbols, call sites and rodata. A program with classes, blocks, gated
//! sites or packages has no sidecar (`--emit-zeodata` refuses it by name),
//! and the subset grows with the front ends that write it.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The whole sidecar. Every list is in the order the code indexes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sidecar {
    /// `zeo_abi::abi::ABI_VERSION` the text was written against.
    pub abi_version: u32,
    /// The read-only bytes the code's rodata offsets index, as hex. The
    /// backend seeds `zeo_rodata` with them verbatim and only appends.
    #[serde(default)]
    pub rodata: String,
    /// The interned symbols, one per `zeo_syms` slot in slot order.
    #[serde(default)]
    pub syms: Vec<String>,
    /// The caller class of every `zeo_callsites` slot, in slot order.
    #[serde(default)]
    pub callsites: Vec<CallerClass>,
    /// The `<main>` body's symbol.
    #[serde(default = "default_toplevel")]
    pub toplevel: String,
    /// The `def`s, in registration order.
    #[serde(default)]
    pub defs: Vec<Def>,
    /// The classes and modules the program defines, in registration order.
    /// Their ids are assigned here, after the last id the empty program
    /// reaches, so a front end never learns one.
    #[serde(default)]
    pub classes: Vec<Class>,
    /// The first class id free for a program's own classes -- the count
    /// the EMPTY program reaches. `--emit-zeodata` writes it and the
    /// backend reads it off its own boot compile; a front end leaves it
    /// out, because the answer belongs to the zeo doing the linking.
    #[serde(default)]
    pub first_user_class: u32,
    /// The builtin class tables the program keeps, by `zeo_ctable_*`
    /// symbol. Two words stand for sets: `"@seed"` is what every program
    /// keeps (the empty program's tables), and `"@all"` is every table this
    /// build carries -- the honest choice for a front end with no
    /// reachability analysis, since a dynamic send can reach any class.
    #[serde(default = "default_class_tables")]
    pub class_tables: Vec<String>,
    /// The registration rows `zeo_rt_main` applies before the first
    /// statement (`zeo_abi::abi::REG_*`). Every program carries the rows
    /// its boot prelude produces -- an `extend`, a builtin registered by
    /// name, an ancestry patch -- and the one entry `"@boot"` is that set:
    /// the rows of the EMPTY program, as this zeo compiles it.
    #[serde(default = "default_reg")]
    pub reg: Vec<RegEntry>,
    /// `$LOAD_PATH`, and how many leading entries a run-time `require`
    /// may search.
    #[serde(default)]
    pub load_path: Vec<String>,
    #[serde(default)]
    pub n_load_path_search: usize,
    /// The parse warnings the program prints at startup.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Whether the program can compile code at run time and so names
    /// `zeo_eval_install`.
    #[serde(default)]
    pub eval_install: bool,
    /// The require-gated builtin features the program loads (`"prism"`,
    /// `"json"`). Each names classes whose ids only this side knows, so
    /// the front end names the FEATURE and the backend registers what it
    /// gates -- concealed, because the constant does not exist until the
    /// `require` runs. `zeo_rt_feature_loaded`, which the front end calls
    /// at the require's position, is what reveals them.
    #[serde(default)]
    pub features: Vec<String>,
    /// The lazily-run feature units. A `require` the front end could not
    /// splice -- one an `if` guards, one inside a method, one whose name
    /// is computed -- keeps its call, and the run-time require looks the
    /// spelling up here and runs the function it names.
    ///
    /// One function answers to several spellings (the name as written and
    /// the absolute path `require_relative` expands to), so a row is one
    /// spelling rather than a set.
    #[serde(default)]
    pub units: Vec<Unit>,
}

/// One spelling a `require` may use, and the function that runs the file
/// it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub feature: String,
    /// A function the text defines, `UnitFn`-shaped: `(out) -> status`.
    pub symbol: String,
}

/// One class or module the program defines. The superclass and the
/// ancestry it implies are resolved by name, so nothing here is an id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Class {
    pub name: String,
    /// Another class in this list, or `Object`. A builtin superclass is
    /// refused: its subclass is a different native shape.
    #[serde(default = "default_superclass")]
    pub superclass: String,
    /// `class` or `module`.
    #[serde(default = "default_class_kind")]
    pub kind: String,
    /// The instance variables the class declares, in slot order.
    #[serde(default)]
    pub ivars: Vec<String>,
    /// The constants the body marked `private_constant`. The owner is an
    /// id the backend assigns, so the front end names them here rather
    /// than writing the registration row itself.
    #[serde(default)]
    pub private_constants: Vec<String>,
    /// Where the class was written, so an error about the ROW -- a name a
    /// builtin already carries, a superclass chain that loops -- names the
    /// line rather than only the class. An empty file is no location.
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub line: u32,
    /// The shape registers before the first statement, so the static MRO
    /// has one, but the CONSTANT stays hidden until the `class` statement
    /// runs and the body calls `zeo_rt_reveal_class`. The id is the
    /// backend's, so the front end asks for the row here rather than
    /// writing it, the same trade `private_constants` makes.
    #[serde(default)]
    pub conceal: bool,
}

/// One `zeo_callsites` slot's caller class: the class whose body the site
/// is written in, which is what ruby's visibility barrier compares an
/// explicit receiver's method against.
///
/// The Rust emitter holds ids and writes one. A front end cannot -- an id
/// is assigned at LINK time -- so it writes the NAME and the backend
/// resolves it, the same trade `Class.superclass` already makes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CallerClass {
    Id(u32),
    /// A class this sidecar declares, `"Object"` for the top level,
    /// `"Class"` or `"Module"` where `self` IS a class, or `""` for
    /// ruby's `VM_CALL_FCALL`: a receiverless call, which asks nothing.
    Named(String),
}

/// One `def`: its dispatch row and its reflection row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Def {
    pub name: String,
    /// The trampoline's symbol -- the `ValueFn` the runtime registers.
    pub tramp: String,
    /// `(kind, name)` in ruby's `Method#parameters` order; kinds are
    /// [`PARAM_KINDS`].
    #[serde(default)]
    pub params: Vec<(String, String)>,
    /// The `def`'s file and line; an empty file is no source location.
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub line: u32,
    /// `public`, `private` or `protected`.
    #[serde(default = "default_visibility")]
    pub visibility: String,
    /// The original name when this row is an alias; empty otherwise.
    #[serde(default)]
    pub aliased_from: String,
    /// The class this method is defined on, by name; empty is the top
    /// level, whose `def`s are private methods of `Object`.
    #[serde(default)]
    pub class: String,
    /// `def self.x` -- the method rides the class-method channel.
    #[serde(default)]
    pub singleton: bool,
}

/// One entry of [`Sidecar::reg`]: a row, or the `"@boot"` set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RegEntry {
    Boot(String),
    Row(RegRow),
}

/// One registration row, the fields of `zeo_abi::abi::RegRow` by name.
/// `f` is a function's symbol, for the kinds that carry one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegRow {
    pub kind: u8,
    pub class: u32,
    #[serde(default)]
    pub a: String,
    #[serde(default)]
    pub b: String,
    #[serde(default)]
    pub f: Option<String>,
    #[serde(default)]
    pub ids: Vec<u32>,
    #[serde(default)]
    pub flag: u8,
}

/// The parameter kinds by name, indexed by `zeo_abi::abi::PARAM_*`.
pub const PARAM_KINDS: &[&str] = &["req", "opt", "rest", "keyreq", "key", "keyrest", "block"];

/// The `@seed` spelling for [`Sidecar::class_tables`].
pub const SEED_TABLES: &str = "@seed";

/// The `@all` spelling for [`Sidecar::class_tables`].
pub const ALL_TABLES: &str = "@all";

/// The `@boot` spelling for [`Sidecar::reg`].
pub const BOOT_ROWS: &str = "@boot";

/// The `@all` spelling for [`Sidecar::features`]: every gated feature
/// this build carries. What a front end names where a `require` builds
/// its feature string at RUN time -- the name is not knowable here, and
/// a gated builtin nothing required literally would register no class at
/// all, leaving the constant a `NameError` after a require that worked.
/// Each is still registered CONCEALED, so the constant appears only when
/// a require actually names it.
pub const ALL_FEATURES: &str = "@all";

fn default_reg() -> Vec<RegEntry> {
    vec![RegEntry::Boot(BOOT_ROWS.to_string())]
}

fn default_toplevel() -> String {
    crate::clif::names::TOPLEVEL.to_string()
}

fn default_class_tables() -> Vec<String> {
    vec![SEED_TABLES.to_string()]
}

fn default_visibility() -> String {
    "public".to_string()
}

fn default_superclass() -> String {
    "Object".to_string()
}

fn default_class_kind() -> String {
    "class".to_string()
}

/// The `superclass` spelling that means the root of the ordinary
/// hierarchy.
pub const OBJECT_SUPERCLASS: &str = "Object";

/// The root BELOW that root: a class naming it inherits neither
/// `Object`'s methods nor `Kernel`'s, which is the whole point of
/// writing it. The only other non-sidecar superclass a class may name.
pub const BASIC_OBJECT_SUPERCLASS: &str = "BasicObject";

/// [`Class::kind`] spellings.
pub const CLASS_KINDS: &[&str] = &["class", "module"];

impl Default for Sidecar {
    /// A program with no symbols, no sites and no `def`s: what a CLIF file
    /// with no sidecar beside it means.
    fn default() -> Sidecar {
        Sidecar {
            abi_version: zeo_abi::abi::ABI_VERSION,
            rodata: String::new(),
            syms: Vec::new(),
            callsites: Vec::new(),
            toplevel: default_toplevel(),
            defs: Vec::new(),
            classes: Vec::new(),
            first_user_class: 0,
            class_tables: default_class_tables(),
            reg: default_reg(),
            load_path: Vec::new(),
            n_load_path_search: 0,
            warnings: Vec::new(),
            eval_install: false,
            features: Vec::new(),
            units: Vec::new(),
        }
    }
}

impl Sidecar {
    pub fn read(path: &Path) -> Result<Sidecar, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).expect("a sidecar serializes");
        std::fs::write(path, json + "\n").map_err(|e| format!("writing {}: {e}", path.display()))
    }

    /// The rodata bytes.
    pub fn rodata_bytes(&self) -> Result<Vec<u8>, String> {
        decode_hex(&self.rodata).map_err(|e| format!("rodata: {e}"))
    }
}

pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("an odd number of hex digits".to_string());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .map_err(|_| format!("`{}` is not a hex byte", &text[i..i + 2]))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(decode_hex(&encode_hex(&bytes)).unwrap(), bytes);
        assert!(decode_hex("abc").is_err());
        assert!(decode_hex("zz").is_err());
    }

    #[test]
    fn an_empty_document_is_the_default_sidecar() {
        let parsed: Sidecar = serde_json::from_str(r#"{"abi_version": 1}"#).unwrap();
        assert_eq!(parsed, Sidecar::default());
        assert_eq!(parsed.class_tables, vec![SEED_TABLES]);
        assert_eq!(parsed.reg, vec![RegEntry::Boot(BOOT_ROWS.to_string())]);
    }

    #[test]
    fn a_class_reads_with_its_defaults() {
        let parsed: Sidecar = serde_json::from_str(
            r#"{"abi_version": 1, "classes": [{"name": "Foo"}],
                "defs": [{"name": "bar", "tramp": "zeo_t_Foo_bar", "class": "Foo"}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.classes[0].superclass, OBJECT_SUPERCLASS);
        assert_eq!(parsed.classes[0].kind, CLASS_KINDS[0]);
        assert!(parsed.classes[0].ivars.is_empty());
        assert!(parsed.classes[0].file.is_empty());
        assert_eq!(parsed.defs[0].class, "Foo");
        assert!(!parsed.defs[0].singleton);
        // The one number a front end must not guess.
        assert_eq!(parsed.first_user_class, 0);
    }

    #[test]
    fn a_registration_row_reads_beside_the_boot_set() {
        let parsed: Sidecar = serde_json::from_str(
            r#"{"abi_version": 1, "reg": ["@boot", {"kind": 4, "class": 49, "ids": [3]}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.reg.len(), 2);
        assert!(matches!(&parsed.reg[1], RegEntry::Row(r) if r.kind == 4 && r.ids == [3]));
    }
}
