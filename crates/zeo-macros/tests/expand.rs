//! Isolated proof that `ruby_class!` emits working code against the runtime
//! contract, without touching `zeo-rt`. The stubs below mirror exactly the
//! symbols the macro references (`crate::RubyValue`, `crate::builtins::*`,
//! `crate::constants::const_set`) so the emitted lookup tables, constant
//! installer, and `linkme` registration are exercised end to end.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RubyValue {
    Nil,
    Int(i64),
}

#[derive(Debug)]
pub struct Signal;

#[derive(Clone, Copy)]
pub struct ClassId(pub u32);

pub const COMPARABLE_CLASS: ClassId = ClassId(22);

pub mod builtins {
    use super::{ClassId, RubyValue, Signal};

    pub type BuiltinMethodFn =
        fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>;

    pub struct MethodTable {
        pub lookup: fn(&str) -> Option<BuiltinMethodFn>,
        pub names: fn() -> &'static [&'static str],
        pub arity: fn(&str) -> Option<i64>,
    }

    pub struct BuiltinClassTable {
        pub id: ClassId,
        pub instance: Option<MethodTable>,
        pub class: Option<MethodTable>,
        pub install_constants: Option<fn()>,
    }

    #[linkme::distributed_slice]
    pub static BUILTIN_TABLES: [BuiltinClassTable] = [..];
}

pub mod constants {
    use super::RubyValue;
    use std::sync::Mutex;

    /// Records `(owner_id, name)` for each seeded constant, so the test can
    /// assert `install_constants` fired.
    pub static SEEDED: Mutex<Vec<(u32, String)>> = Mutex::new(Vec::new());

    pub fn const_set(owner: u32, name: &str, _value: RubyValue) {
        SEEDED.lock().unwrap().push((owner, name.to_string()));
    }
}

mod comparable {
    use crate::RubyValue;

    zeo_macros::ruby_module! {
        Comparable = crate::COMPARABLE_CLASS;

        const SENTINEL = RubyValue::Int(7);

        def "<" arity 1 (recv, _args, _block) {
            let _ = recv;
            Ok(RubyValue::Int(-1))
        }
        def "between?" arity 2 (_recv, _args, _block) {
            Ok(RubyValue::Nil)
        }
        def self."probe"(_recv, _args, _block) {
            Ok(RubyValue::Int(99))
        }

        alias lteq = "<";
    }
}

#[test]
fn instance_lookup_arity_and_names() {
    // Operator + `?` names resolve through the generated instance table.
    let lt = comparable::lookup("<").expect("`<` is defined");
    assert_eq!(lt(&RubyValue::Nil, &[], None).unwrap(), RubyValue::Int(-1));
    assert!(comparable::lookup("between?").is_some());
    assert!(comparable::lookup("nope").is_none());

    // Per-name arity, defaulting handled elsewhere.
    assert_eq!(comparable::lookup_arity("<"), Some(1));
    assert_eq!(comparable::lookup_arity("between?"), Some(2));
    assert_eq!(comparable::lookup_arity("nope"), None);

    // Reflection names enumerate every alias.
    let names = comparable::lookup_names();
    assert!(names.contains(&"<"));
    assert!(names.contains(&"between?"));
    assert!(names.contains(&"lteq"));
}

#[test]
fn late_alias_shares_impl_and_arity() {
    // `lteq` aliases `<`: same behavior, same arity, distinct name.
    let lteq = comparable::lookup("lteq").expect("alias registered");
    assert_eq!(lteq(&RubyValue::Nil, &[], None).unwrap(), RubyValue::Int(-1));
    assert_eq!(comparable::lookup_arity("lteq"), Some(1));
}

#[test]
fn class_methods_live_in_a_separate_table() {
    let probe = comparable::lookup_class("probe").expect("`self.probe` defined");
    assert_eq!(probe(&RubyValue::Nil, &[], None).unwrap(), RubyValue::Int(99));
    // Class name is not an instance method, and vice versa.
    assert!(comparable::lookup("probe").is_none());
    assert!(comparable::lookup_class("<").is_none());
}

#[test]
fn constants_install_through_the_thunk() {
    comparable::install_constants();
    let seeded = crate::constants::SEEDED.lock().unwrap();
    assert!(
        seeded.iter().any(|(owner, name)| *owner == 22 && name == "SENTINEL"),
        "expected SENTINEL seeded under COMPARABLE_CLASS(22), got {seeded:?}"
    );
}

#[test]
fn linkme_registers_the_table_keyed_by_class_id() {
    let entry = crate::builtins::BUILTIN_TABLES
        .iter()
        .find(|t| t.id.0 == 22)
        .expect("Comparable registered into BUILTIN_TABLES");
    assert!(entry.instance.is_some(), "has an instance table");
    assert!(entry.class.is_some(), "has a class-method table");
    assert!(entry.install_constants.is_some(), "has a constant installer");

    // The registered fn pointers behave identically to the module's.
    let lookup = entry.instance.as_ref().unwrap().lookup;
    assert!(lookup("between?").is_some());
}
