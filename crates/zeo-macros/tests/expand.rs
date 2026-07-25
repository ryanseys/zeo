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
pub const NESTED_CLASS: ClassId = ClassId(23);
pub const OBJECT_CLASS: ClassId = ClassId(0);

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

        // A `def ... as X` binds a callable Rust fn name, so a sibling body can
        // call it DIRECTLY (not through the table). `clamp` calls `cmp_impl`.
        def "cmp" as cmp_impl (_recv, _args, _block) {
            Ok(RubyValue::Int(5))
        }
        def "clamp" (recv, args, block) {
            cmp_impl(recv, args, block)
        }

        // A `module_function` is emitted into BOTH the instance and class
        // tables (like `Math.sqrt` / `include Math; sqrt`).
        module_function def "mf" (_recv, _args, _block) {
            Ok(RubyValue::Int(77))
        }

        // `#[cfg(...)]` on a `def` gates the fn AND its lookup/names/arity rows
        // as a unit (a platform-specific accessor). `all()` is always true,
        // `any()` always false -- so `cfg_in` is present and `cfg_out` is not,
        // regardless of target.
        #[cfg(all())]
        def "cfg_in" arity 0 (_recv, _args, _block) {
            Ok(RubyValue::Int(1))
        }
        #[cfg(any())]
        def "cfg_out" (_recv, _args, _block) {
            Ok(RubyValue::Int(2))
        }

        // A nested class sharing the same file: its own id, its own `lookup`
        // table (in a private submodule, so no collision with the outer one).
        class Nested = crate::NESTED_CLASS < crate::OBJECT_CLASS {
            def "ping"(_recv, _args, _block) {
                Ok(RubyValue::Int(42))
            }
        }
    }
}

#[test]
fn a_bound_name_is_callable_by_its_rust_name_and_via_the_table() {
    // Direct Rust call by the bound name -- the point of `as X`: a sibling body
    // (`clamp`) reached `cmp_impl` directly, and it's callable from here too.
    assert_eq!(
        comparable::cmp_impl(&RubyValue::Nil, &[], None).unwrap(),
        RubyValue::Int(5)
    );
    // The `clamp` row, which delegates to `cmp_impl`, resolves through the table
    // and returns the same thing -- proving the direct call and the Ruby-name
    // dispatch reach one shared implementation.
    let clamp = comparable::lookup("clamp").expect("`clamp` defined");
    assert_eq!(clamp(&RubyValue::Nil, &[], None).unwrap(), RubyValue::Int(5));
    // And the bound name is still reachable by its Ruby name "cmp".
    assert!(comparable::lookup("cmp").is_some());
}

#[test]
fn a_module_function_lands_in_both_the_instance_and_class_tables() {
    // `mf` resolves as an instance method AND as a class method, both running
    // the one shared body -- CRuby's `module_function` shape.
    let inst = comparable::lookup("mf").expect("mf is an instance method");
    assert_eq!(inst(&RubyValue::Nil, &[], None).unwrap(), RubyValue::Int(77));
    let cls = comparable::lookup_class("mf").expect("mf is a class method");
    assert_eq!(cls(&RubyValue::Nil, &[], None).unwrap(), RubyValue::Int(77));
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
fn a_cfg_on_a_def_gates_the_fn_and_its_table_rows_together() {
    // The always-true cfg keeps `cfg_in` in the lookup, names, and arity tables.
    let cfg_in = comparable::lookup("cfg_in").expect("cfg_in present under cfg(all())");
    assert_eq!(cfg_in(&RubyValue::Nil, &[], None).unwrap(), RubyValue::Int(1));
    assert!(comparable::lookup_names().contains(&"cfg_in"));
    assert_eq!(comparable::lookup_arity("cfg_in"), Some(0));

    // The always-false cfg drops `cfg_out` from every surface.
    assert!(comparable::lookup("cfg_out").is_none());
    assert!(!comparable::lookup_names().contains(&"cfg_out"));
    assert_eq!(comparable::lookup_arity("cfg_out"), None);
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
fn nested_class_registers_separately_without_colliding() {
    // The nested `Nested` class gets its OWN BUILTIN_TABLES entry, keyed by its
    // own id, distinct from the outer Comparable module -- proof that two
    // classes in one file don't clobber each other's fixed table fn names.
    let outer = crate::builtins::BUILTIN_TABLES
        .iter()
        .find(|t| t.id.0 == 22)
        .expect("Comparable registered");
    let nested = crate::builtins::BUILTIN_TABLES
        .iter()
        .find(|t| t.id.0 == 23)
        .expect("Nested registered under its own id");

    // The nested instance method resolves and runs.
    let ping = nested
        .instance
        .as_ref()
        .expect("Nested has an instance table")
        .lookup;
    assert_eq!(
        ping("ping").expect("`ping` defined")(&RubyValue::Nil, &[], None).unwrap(),
        RubyValue::Int(42)
    );

    // The two tables are genuinely distinct: the outer's `<` is not on the
    // nested table, and the nested's `ping` is not on the outer.
    assert!((outer.instance.as_ref().unwrap().lookup)("ping").is_none());
    assert!((nested.instance.as_ref().unwrap().lookup)("<").is_none());
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
