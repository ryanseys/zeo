//! The package row manifest -- a separately compiled gem's registration
//! rows as DATA.
//!
//! A package compile (`--experimental-pkg`) emits an object file whose
//! bodies are exported symbols, plus this manifest: every row its
//! `ProgramDesc` would have carried, with function pointers named by
//! SYMBOL. A host compile (`--experimental-use-pkg`) rewrites the rows into
//! its own single desc and links the package object beside its own, so the
//! runtime still registers exactly one program. See the plan's Part III
//! (decision 9: link-time merge, one desc, an untouched runtime).
//!
//! Class ids in a manifest are FINAL: the package minted its classes
//! densely right after the shared bootstrap, and the host pads its own
//! minting past the recorded band, so both objects' baked immediates are
//! correct without rewriting. The id-translation tier that makes a package
//! position-independent replaces this scheme in M2.

use serde::{Deserialize, Serialize};

/// Bumped when the manifest schema changes shape. Independent of
/// `zeo_abi::ABI_VERSION`: the manifest is a compiler-to-compiler file.
pub const MANIFEST_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub abi_version: u32,
    /// The symbol prefix this package's exported unit machinery carries
    /// (`zeo_pkg_<name>`); bodies keep their natural names until M1.
    pub prefix: String,
    /// The feature spelling the package was built for (`require "<this>"`).
    pub feature: String,
    /// The first class id this package minted -- the shared bootstrap's
    /// class count, which the host ASSERTS before padding.
    pub first_class_id: u32,
    /// Ids `[first_class_id, first_class_id + n_class_ids)` belong to this
    /// package (box surrogates included).
    pub n_class_ids: u32,
    /// Reveal-group ids `[0, n_units)` belong to this package: its feature
    /// units plus its alias-reveal groups share the space, and the host
    /// offsets its own by the total.
    pub n_units: u32,
    pub classes: Vec<MClass>,
    pub vm: Vec<MVmRow>,
    pub vis: Vec<MVisRow>,
    pub obj: Vec<MObjRow>,
    pub cm: Vec<MCmRow>,
    pub reg: Vec<MRegRow>,
    pub foreign: Vec<(u32, String)>,
    pub meta: Vec<MMetaRow>,
    /// `(feature spelling, unit fn symbol)` -- both spellings a require can
    /// build, exactly as `UnitRow`s carry them.
    pub units: Vec<(String, String)>,
    /// The package object's exported site-table initializer, run from the
    /// host's own `zeo_unit_init`.
    pub unit_init: Option<String>,
    /// The caller-class id per inline-cache slot (`u32::MAX` = an FCALL
    /// with no visibility question), in LOCAL ids. The package object
    /// IMPORTS `{prefix}_callers`; the host defines it with the band it
    /// assigned.
    pub callers: Vec<u32>,
    /// The builtin dispatch tables this package's code can reach, by
    /// `zeo_ctable_*` name -- unioned into the host's list so
    /// `-dead_strip` keeps them.
    pub class_tables: Vec<String>,
    /// What the package DOES to the shared world -- see [`MFacts`]. A host
    /// unions these with its own arena facts before any fold fires.
    pub facts: MFacts,
    /// What the package IS, for the host's COMPILE: enough per class to
    /// register a real `ClassInfo` (body-less scopes) so host code resolves
    /// constants, infers receiver types, and nominates typed direct calls
    /// into the package's exported bodies. Joined with `classes` by id.
    pub iface: Vec<MIfaceClass>,
}

/// One class's compile-time interface. `parent`/`mixin_order`/`extends`
/// are LOCAL ids (or builtin ids below the package band); the host
/// re-materializes the MRO from them with the same algorithm the package
/// ran, so the two worlds cannot disagree.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MIfaceClass {
    pub id: u32,
    pub parent: Option<u32>,
    pub is_module: bool,
    /// `(module id, prepend?)` in declaration order.
    pub mixin_order: Vec<(u32, bool)>,
    pub extends: Vec<u32>,
    pub hidden_ivars: Vec<String>,
    pub methods: Vec<MIfaceMethod>,
    pub class_methods: Vec<MIfaceMethod>,
}

/// One own method. `body` is the exported BODY symbol when the package
/// compiled a plain direct-callable body for it (`None` keeps the method
/// lookup-visible but dispatch-only). The shape numbers reproduce the
/// package's own `params::layout_of` verdict, so the host's synthesized
/// scope cannot disagree with the compiled body's ABI.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MIfaceMethod {
    pub name: String,
    /// 0 = public, 1 = private, 2 = protected.
    pub visibility: u8,
    pub plain: bool,
    /// Required-positional count (only meaningful when `plain`).
    pub arity: u32,
    pub has_blk: bool,
    pub runtime_conditional: bool,
    pub body: Option<String>,
}

/// The package's whole-program facts, extracted from the same `Compiler`
/// state the arena sweep fills. Every field is MONOTONE-CONSERVATIVE: a
/// host that unions them can only fold less, never differently -- which is
/// the correct failure direction for a fact someone forgets to carry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MFacts {
    /// Method names a runtime site in the package could (re)define or
    /// re-scope -- `Compiler::runtime_patches`, sorted.
    pub patched_names: Vec<String>,
    /// A patch whose NAME is computed (`define_method(sym)`), which
    /// suppresses every name-keyed fold at once.
    pub patches_any_name: bool,
    /// `Compiler::program_freezes`.
    pub freezes: bool,
    /// The package gives `!` a body somewhere.
    pub defines_bang: bool,
    /// A BasicObject-rooted receiver is possible in the package.
    pub blank_slate_possible: bool,
    /// A Ractor-moved husk is possible in the package.
    pub moved_receiver_possible: bool,
    /// Every constant name the package can assign when it loads --
    /// `Compiler::assigned_const_names`, sorted. The host files them under
    /// `unrun_unit_consts`, so `defined?` and the const folds ask the run
    /// time instead of deciding from a body the host cannot see.
    pub const_names: Vec<String>,
    /// Global variables the package writes, sorted.
    pub global_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MClass {
    pub id: u32,
    pub name: String,
    pub ancestors: Vec<u32>,
    pub ivars: Vec<String>,
    pub hidden: u16,
    pub members: Vec<String>,
    pub kind: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MVmRow {
    pub class: u32,
    pub box_id: u32,
    pub name: String,
    pub f: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MVisRow {
    pub class: u32,
    pub name: String,
    pub verb: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MObjRow {
    pub class: u32,
    pub name: String,
    pub f: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MCmRow {
    pub class: u32,
    pub box_id: u32,
    pub name: String,
    pub f: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MRegRow {
    pub kind: u8,
    pub class: u32,
    pub a: String,
    pub b: String,
    pub f: Option<String>,
    pub ids: Vec<u32>,
    pub flag: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MMetaRow {
    pub class: u32,
    pub singleton: bool,
    pub name: String,
    pub params: Vec<(u8, String)>,
    pub file: String,
    pub line: u32,
    pub aliased_from: String,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let m: Manifest =
            serde_json::from_str(text).map_err(|e| format!("reading a package manifest: {e}"))?;
        if m.manifest_version != MANIFEST_VERSION {
            return Err(format!(
                "package manifest version {} does not match this zeo's {MANIFEST_VERSION}; \
                 rebuild the package",
                m.manifest_version
            ));
        }
        if m.abi_version != zeo_abi::abi::ABI_VERSION {
            return Err(format!(
                "package ABI version {} does not match this zeo's {}; rebuild the package",
                m.abi_version,
                zeo_abi::abi::ABI_VERSION
            ));
        }
        Ok(m)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a manifest serializes")
    }
}

/// A package named on the host command line: its manifest (content read up
/// front, so the compile is a function of the TEXT, not the path) and the
/// object file to link beside the host's own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UsePackage {
    pub manifest_path: std::path::PathBuf,
    pub manifest_text: String,
    pub object: std::path::PathBuf,
    /// A digest of the object file's bytes. Carried so the program cache's
    /// options hash changes when the package is rebuilt: the manifest text
    /// alone misses a body-only change.
    pub object_digest: u64,
}

/// The package build a compile was asked for (`--experimental-pkg`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageBuild {
    /// The gem's entry file, compiled as a FEATURE UNIT under `feature`.
    pub entry: std::path::PathBuf,
    /// The feature spelling a host will require.
    pub feature: String,
    /// Where the manifest lands, beside the object.
    pub manifest_out: std::path::PathBuf,
}

impl PackageBuild {
    /// The exported-symbol prefix: the feature spelling with every
    /// non-identifier byte folded to `_`.
    pub fn prefix(&self) -> String {
        let safe: String = self
            .feature
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("zeo_pkg_{safe}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            manifest_version: MANIFEST_VERSION,
            abi_version: zeo_abi::abi::ABI_VERSION,
            prefix: "zeo_pkg_pureleaf".into(),
            feature: "pureleaf".into(),
            first_class_id: 400,
            n_class_ids: 2,
            n_units: 1,
            classes: vec![MClass {
                id: 400,
                name: "Pureleaf".into(),
                ancestors: vec![400, 0],
                ivars: vec!["@x".into()],
                hidden: 0,
                members: vec![],
                kind: 0,
            }],
            vm: vec![],
            vis: vec![],
            obj: vec![MObjRow {
                class: 400,
                name: "leaf".into(),
                f: "zeo_t_Pureleaf_leaf".into(),
            }],
            cm: vec![],
            reg: vec![MRegRow {
                kind: zeo_abi::abi::REG_MARK_OWN_ROWS,
                class: 400,
                a: "leaf".into(),
                b: String::new(),
                f: None,
                ids: vec![],
                flag: 0,
            }],
            foreign: vec![],
            meta: vec![],
            units: vec![("pureleaf".into(), "zeo_pkg_pureleaf_unit_0".into())],
            unit_init: Some("zeo_pkg_pureleaf_unit_init".into()),
            callers: vec![u32::MAX, 400],
            class_tables: vec![],
            facts: MFacts {
                const_names: vec!["PURELEAF_TAG".into()],
                ..MFacts::default()
            },
            iface: vec![MIfaceClass {
                id: 400,
                parent: Some(0),
                methods: vec![MIfaceMethod {
                    name: "leaf".into(),
                    plain: true,
                    body: Some("zeo_m_Pureleaf_leaf".into()),
                    ..MIfaceMethod::default()
                }],
                ..MIfaceClass::default()
            }],
        }
    }

    #[test]
    fn a_manifest_round_trips() {
        let m = sample();
        assert_eq!(Manifest::parse(&m.to_json()).unwrap(), m);
    }

    #[test]
    fn a_version_mismatch_is_refused_by_name() {
        let mut m = sample();
        m.manifest_version += 1;
        let err = Manifest::parse(&m.to_json()).unwrap_err();
        assert!(err.contains("manifest version"), "{err}");
    }
}
