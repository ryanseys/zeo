//! Unit tests for the dispatch machinery: the MRO walk, `super` resume, the
//! singleton chain, the visibility barrier, `method_missing`, and the inline
//! caches.
//!
//! Every test here mutates process-global state (the `REGISTRY` `OnceLock`,
//! the gate word), so each depends on nextest's process-per-test isolation --
//! see the GATES contract in `runtime_meta` and the crate README. A test that
//! needs real classes installs `ClassRegistry::with_core()` plus its own user
//! rows, the same shape a generated `main()` registers.

use super::*;
use crate::RProc;

/// Build the core world, let `build` layer user rows on top, and install --
/// the bootstrap every registry-backed test starts with.
fn install_core_with(build: impl FnOnce(&mut ClassRegistry)) {
    let mut registry = ClassRegistry::with_core();
    build(&mut registry);
    install_class_registry(registry);
}

/// Register a user CLASS: `[id, supers.., Object, Kernel, BasicObject]`,
/// already linearized, the way codegen registers one.
fn user_class(registry: &mut ClassRegistry, id: u32, name: &str, supers: &[ClassId]) -> ClassId {
    let cid = ClassId(id);
    let mut ancestors = vec![cid];
    ancestors.extend_from_slice(supers);
    ancestors.extend_from_slice(&[zeo_abi::OBJECT_CLASS, KERNEL_CLASS, BASIC_OBJECT_CLASS]);
    registry.register(cid, name, false, ancestors, None);
    cid
}

/// Register a user MODULE (ancestors are just itself, like a compiled one).
fn user_module(registry: &mut ClassRegistry, id: u32, name: &str) -> ClassId {
    let mid = ClassId(id);
    registry.register(mid, name, true, vec![mid], None);
    mid
}

fn instance_of(cid: ClassId) -> RubyValue {
    RubyValue::Object(crate::runtime_meta::dyn_alloc(cid))
}

fn int_of(r: Result<RubyValue, Signal>) -> i64 {
    match r {
        Ok(RubyValue::Int(n)) => n,
        Ok(_) => panic!("expected an Int result"),
        Err(_) => panic!("expected Ok, got a signal"),
    }
}

/// The raised exception's class id and message -- asserts the error really is
/// a rescuable `Signal::Raise`, not a panic.
fn raised(r: Result<RubyValue, Signal>) -> (ClassId, String) {
    let Err(Signal::Raise(exc)) = r else {
        panic!("expected a raised exception");
    };
    let class = exc.as_object_unchecked().class_id();
    let msg = match send_value(&exc, Symbol::intern("message"), &[], None) {
        Ok(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
        _ => panic!("#message must answer a String"),
    };
    (class, msg)
}

/// One instance-channel row per distinct answer -- `MethodFn` needs real fns.
macro_rules! obj_row {
    ($name:ident, $n:expr) => {
        fn $name(_: &RObj, _: &[RubyValue], _: Option<RubyValue>) -> Result<RubyValue, Signal> {
            Ok(RubyValue::Int($n))
        }
    };
}
obj_row!(row_1, 1);
obj_row!(row_2, 2);
obj_row!(row_3, 3);
obj_row!(row_5, 5);
obj_row!(row_7, 7);

/// The class-method (value-channel) twin.
macro_rules! class_row {
    ($name:ident, $n:expr) => {
        fn $name(
            _: &RubyValue,
            _: &[RubyValue],
            _: Option<RubyValue>,
        ) -> Result<RubyValue, Signal> {
            Ok(RubyValue::Int($n))
        }
    };
}
class_row!(cm_10, 10);
class_row!(cm_20, 20);

// ---------------------------------------------------------------------------
// The MRO walk
// ---------------------------------------------------------------------------

#[test]
fn an_instance_resolves_a_method_its_ancestor_defines() {
    let base = ClassId(500_001);
    let sub = ClassId(500_002);
    let greet = Symbol::intern("greet");
    install_core_with(|r| {
        user_class(r, base.0, "Base", &[]);
        user_class(r, sub.0, "Sub", &[base]);
        r.define_method(base, greet, row_1);
    });

    // Sub's own table is empty; the walk finds Base's row.
    assert_eq!(int_of(send_value(&instance_of(sub), greet, &[], None)), 1);
}

#[test]
fn an_own_row_shadows_the_ancestors_copy() {
    let base = ClassId(500_001);
    let sub = ClassId(500_002);
    let greet = Symbol::intern("greet");
    install_core_with(|r| {
        user_class(r, base.0, "Base", &[]);
        user_class(r, sub.0, "Sub", &[base]);
        r.define_method(base, greet, row_1);
        r.define_method(sub, greet, row_2);
    });

    assert_eq!(int_of(send_value(&instance_of(sub), greet, &[], None)), 2);
    // The ancestor still answers for its own instances.
    assert_eq!(int_of(send_value(&instance_of(base), greet, &[], None)), 1);
}

#[test]
fn an_included_module_wins_between_self_and_superclass() {
    let base = ClassId(500_001);
    let mixin = ClassId(500_002);
    let sub = ClassId(500_003);
    let both = Symbol::intern("both");
    install_core_with(|r| {
        user_class(r, base.0, "Base", &[]);
        user_module(r, mixin.0, "Mixin");
        // The linearized chain seats the include between self and the super.
        user_class(r, sub.0, "Sub", &[mixin, base]);
        r.define_method(base, both, row_1);
        r.define_method(mixin, both, row_2);
    });

    assert!(ancestors_of_value(sub).contains(&mixin));
    // The module's copy sits nearer than the superclass's.
    assert_eq!(int_of(send_value(&instance_of(sub), both, &[], None)), 2);
}

// ---------------------------------------------------------------------------
// super resume
// ---------------------------------------------------------------------------

#[test]
fn super_resumes_past_the_defining_class_at_each_hop() {
    let top = ClassId(500_001);
    let mid = ClassId(500_002);
    let sub = ClassId(500_003);
    let title = Symbol::intern("title");
    install_core_with(|r| {
        user_class(r, top.0, "Top", &[]);
        user_class(r, mid.0, "Mid", &[top]);
        user_class(r, sub.0, "Sub", &[mid, top]);
        // `define_method_own` fills `own_impls`, the set the super walk reads.
        r.define_method_own(top, title, row_3);
        r.define_method_own(mid, title, row_2);
        r.define_method_own(sub, title, row_1);
    });
    let recv = instance_of(sub);

    // Ordinary dispatch lands on the first copy.
    assert_eq!(int_of(send_value(&recv, title, &[], None)), 1);
    // A `super` written in Sub resumes at Mid; one written in Mid at Top.
    assert_eq!(int_of(send_super_from(&recv, sub, title, &[], None)), 2);
    assert_eq!(int_of(send_super_from(&recv, mid, title, &[], None)), 3);
}

#[test]
fn super_past_the_last_definition_raises_no_method_error() {
    let top = ClassId(500_001);
    let sub = ClassId(500_002);
    let title = Symbol::intern("title");
    install_core_with(|r| {
        user_class(r, top.0, "Top", &[]);
        user_class(r, sub.0, "Sub", &[top]);
        r.define_method_own(top, title, row_1);
        r.define_method_own(sub, title, row_2);
    });
    let recv = instance_of(sub);

    let (class, msg) = raised(send_super_from(&recv, top, title, &[], None));
    assert_eq!(class, zeo_abi::NO_METHOD_ERROR_CLASS);
    assert!(
        msg.starts_with("super: no superclass method 'title'"),
        "{msg}"
    );
}

// ---------------------------------------------------------------------------
// The singleton chain
// ---------------------------------------------------------------------------

#[test]
fn a_class_method_resolves_down_the_singleton_chain() {
    let base = ClassId(500_001);
    let sub = ClassId(500_002);
    let make = Symbol::intern("make");
    install_core_with(|r| {
        user_class(r, base.0, "Base", &[]);
        user_class(r, sub.0, "Sub", &[base]);
        r.define_class_method(base, make, cm_10);
        r.mark_own_class_method(base, make);
    });

    // Sub has no own row; the walk's Own(Sub) position misses and Own(Base)
    // answers -- the class-then-singleton-ancestors order.
    assert_eq!(int_of(send_class_chain(sub, make, &[], None)), 10);
    assert_eq!(class_method_owner(sub, make), Some(base));
}

#[test]
fn a_class_method_super_resumes_past_the_own_position() {
    let base = ClassId(500_001);
    let sub = ClassId(500_002);
    let make = Symbol::intern("make");
    install_core_with(|r| {
        user_class(r, base.0, "Base", &[]);
        user_class(r, sub.0, "Sub", &[base]);
        r.define_class_method(base, make, cm_10);
        r.mark_own_class_method(base, make);
        r.define_class_method(sub, make, cm_20);
        r.mark_own_class_method(sub, make);
    });

    // Ordinary class-method dispatch lands on the receiver's own row.
    assert_eq!(int_of(send_class_chain(sub, make, &[], None)), 20);
    // A `super` written in Sub's `def self.make` resumes at Base's.
    assert_eq!(int_of(send_super_class_from(sub, sub, make, &[], None)), 10);
}

// ---------------------------------------------------------------------------
// The visibility barrier
// ---------------------------------------------------------------------------

#[test]
fn a_private_row_refuses_an_explicit_receiver() {
    let vault = ClassId(500_001);
    let caller = ClassId(500_002);
    let secret = Symbol::intern("secret");
    install_core_with(|r| {
        user_class(r, vault.0, "Vault", &[]);
        user_class(r, caller.0, "Caller", &[]);
        r.define_method(vault, secret, row_1);
        r.mark_private(vault, secret);
    });

    let err = send_value_explicit_in(0, &instance_of(vault), secret, &[], None, caller.0);
    let (class, msg) = raised(err);
    assert_eq!(class, zeo_abi::NO_METHOD_ERROR_CLASS);
    assert!(msg.starts_with("private method 'secret'"), "{msg}");
}

#[test]
fn a_private_row_allows_an_fcall_caller() {
    let vault = ClassId(500_001);
    let secret = Symbol::intern("secret");
    install_core_with(|r| {
        user_class(r, vault.0, "Vault", &[]);
        r.define_method(vault, secret, row_1);
        r.mark_private(vault, secret);
    });

    // FCALL is the implicit-receiver / literal-self / `send` mode: no
    // visibility question at all.
    let ok = send_value_explicit_in(0, &instance_of(vault), secret, &[], None, FCALL);
    assert_eq!(int_of(ok), 1);
}

#[test]
fn a_protected_row_allows_kin_and_refuses_strangers() {
    let owner = ClassId(500_001);
    let kin = ClassId(500_002);
    let stranger = ClassId(500_003);
    let family = Symbol::intern("family");
    install_core_with(|r| {
        user_class(r, owner.0, "Owner", &[]);
        user_class(r, kin.0, "Kin", &[owner]);
        user_class(r, stranger.0, "Stranger", &[]);
        r.define_method(owner, family, row_5);
        r.mark_protected(owner, family);
        // The barrier resolves the owner through the direct-def set.
        r.mark_own(owner, family);
    });
    let recv = instance_of(owner);

    // A caller whose class descends from the owner is kin.
    assert_eq!(
        int_of(send_value_explicit_in(0, &recv, family, &[], None, kin.0)),
        5
    );
    let (class, msg) = raised(send_value_explicit_in(
        0,
        &recv,
        family,
        &[],
        None,
        stranger.0,
    ));
    assert_eq!(class, zeo_abi::NO_METHOD_ERROR_CLASS);
    assert!(msg.starts_with("protected method 'family'"), "{msg}");
}

// ---------------------------------------------------------------------------
// method_missing
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_name_raises_no_method_error() {
    let plain = ClassId(500_001);
    install_core_with(|r| {
        user_class(r, plain.0, "Plain", &[]);
    });

    let err = send_value(
        &instance_of(plain),
        Symbol::intern("nonexistent"),
        &[],
        None,
    );
    let (class, msg) = raised(err);
    assert_eq!(class, zeo_abi::NO_METHOD_ERROR_CLASS);
    assert!(msg.starts_with("undefined method 'nonexistent'"), "{msg}");
}

#[test]
fn a_user_method_missing_hook_intercepts_the_miss() {
    fn echo_name(_: &RObj, args: &[RubyValue], _: Option<RubyValue>) -> Result<RubyValue, Signal> {
        // CRuby's protocol: the missing name arrives prepended to the args.
        Ok(args.first().cloned().unwrap_or(RubyValue::Nil))
    }
    let hooked = ClassId(500_001);
    install_core_with(|r| {
        user_class(r, hooked.0, "Hooked", &[]);
        r.define_method(hooked, Symbol::intern("method_missing"), echo_name);
    });

    let out = send_value(&instance_of(hooked), Symbol::intern("whatever"), &[], None);
    assert!(matches!(out, Ok(RubyValue::Symbol(s)) if s == Symbol::intern("whatever")));
}

// ---------------------------------------------------------------------------
// The inline caches
// ---------------------------------------------------------------------------

#[test]
fn a_call_site_fills_once_and_hits_the_same_class() {
    let a = ClassId(500_001);
    let ping = Symbol::intern("ping");
    install_core_with(|r| {
        user_class(r, a.0, "A", &[]);
        r.define_method(a, ping, row_1);
    });
    static SITE: CallSite = CallSite::new(FCALL);
    let recv = instance_of(a);

    assert_eq!(SITE.cached_class(), None);
    // The first send resolves, fills, and answers.
    assert_eq!(
        int_of(send_value_cached(&SITE, 0, &recv, ping, &[], None)),
        1
    );
    assert_eq!(SITE.cached_class(), Some(a.0));
    // The second is a hit on the same class.
    assert_eq!(
        int_of(send_value_cached(&SITE, 0, &recv, ping, &[], None)),
        1
    );
    assert_eq!(SITE.cached_class(), Some(a.0));
}

#[test]
fn a_second_class_misses_and_the_first_fill_stays() {
    let a = ClassId(500_001);
    let b = ClassId(500_002);
    let ping = Symbol::intern("ping");
    install_core_with(|r| {
        user_class(r, a.0, "A", &[]);
        user_class(r, b.0, "B", &[]);
        r.define_method(a, ping, row_1);
        r.define_method(b, ping, row_2);
    });
    static SITE: CallSite = CallSite::new(FCALL);

    assert_eq!(
        int_of(send_value_cached(
            &SITE,
            0,
            &instance_of(a),
            ping,
            &[],
            None
        )),
        1
    );
    // Filled ONCE and never replaced: the second class still answers
    // correctly (through the uncached route) and the fill is untouched.
    assert_eq!(
        int_of(send_value_cached(
            &SITE,
            0,
            &instance_of(b),
            ping,
            &[],
            None
        )),
        2
    );
    assert_eq!(SITE.cached_class(), Some(a.0));
}

#[test]
fn a_live_overlay_turns_a_filled_site_off() {
    let a = ClassId(500_001);
    let ping = Symbol::intern("ping");
    install_core_with(|r| {
        user_class(r, a.0, "A", &[]);
        r.define_method(a, ping, row_1);
    });
    static SITE: CallSite = CallSite::new(FCALL);
    let recv = instance_of(a);
    assert_eq!(
        int_of(send_value_cached(&SITE, 0, &recv, ping, &[], None)),
        1
    );
    assert_eq!(SITE.cached_class(), Some(a.0));

    // A runtime redefinition arms `is_live`, which turns the cache off
    // wholesale -- no invalidation edge, the gate IS the invalidation.
    crate::runtime_meta::runtime_define_method(
        a,
        ping,
        RProc::with_meta(|_| Ok(RubyValue::Int(3)), 0, false),
    )
    .unwrap();
    assert_eq!(
        int_of(send_value_cached(&SITE, 0, &recv, ping, &[], None)),
        3
    );
    // The stale fill is still sitting there, gated off -- never served.
    assert_eq!(SITE.cached_class(), Some(a.0));
}

#[test]
fn a_class_method_site_remembers_a_hit_and_a_miss() {
    let c = ClassId(500_001);
    let make = Symbol::intern("make");
    install_core_with(|r| {
        user_class(r, c.0, "C", &[]);
        r.define_class_method(c, make, cm_10);
        r.mark_own_class_method(c, make);
    });
    let recv = RubyValue::Class(c);

    // A resolved row fills and hits.
    static HIT_SITE: ClassMethodSite = ClassMethodSite::new();
    assert_eq!(HIT_SITE.cached_caller(), None);
    assert_eq!(
        int_of(send_class_cached(
            &HIT_SITE,
            c.0,
            &recv,
            make,
            &[],
            None,
            FCALL
        )),
        10
    );
    assert_eq!(HIT_SITE.cached_caller(), Some(FCALL));
    assert_eq!(HIT_SITE.cached_is_miss(), Some(false));
    assert_eq!(
        int_of(send_class_cached(
            &HIT_SITE,
            c.0,
            &recv,
            make,
            &[],
            None,
            FCALL
        )),
        10
    );

    // A name the flat probe cannot serve (`Module#name` resolves through the
    // MRO walk) records a remembered MISS and still answers.
    static MISS_SITE: ClassMethodSite = ClassMethodSite::new();
    let name = Symbol::intern("name");
    let out = send_class_cached(&MISS_SITE, c.0, &recv, name, &[], None, FCALL);
    assert!(matches!(&out, Ok(RubyValue::Str(s)) if s.lock().to_utf8_lossy() == "C"));
    assert_eq!(MISS_SITE.cached_is_miss(), Some(true));
}

#[test]
fn a_dyn_site_denies_before_fill_and_vets_after() {
    let vault = ClassId(500_001);
    let stranger = ClassId(500_002);
    let secret = Symbol::intern("secret");
    install_core_with(|r| {
        user_class(r, vault.0, "Vault", &[]);
        user_class(r, stranger.0, "Stranger", &[]);
        r.define_method(vault, secret, row_7);
        r.mark_private(vault, secret);
    });
    static SITE: DynCallerSite = DynCallerSite::new();
    let recv = instance_of(vault);

    // A denied call fills NOTHING: the deny is re-asked per call, the shape
    // the uncached path has.
    let (class, _) = raised(send_value_dyn_cached(
        &SITE,
        0,
        &recv,
        secret,
        &[],
        None,
        stranger.0,
    ));
    assert_eq!(class, zeo_abi::NO_METHOD_ERROR_CLASS);
    assert_eq!(SITE.cached_class(), None);

    // An FCALL caller passes the vet, fills, and answers.
    assert_eq!(
        int_of(send_value_dyn_cached(
            &SITE,
            0,
            &recv,
            secret,
            &[],
            None,
            FCALL
        )),
        7
    );
    assert_eq!(SITE.cached_class(), Some(vault.0));

    // The filled site still asks the caller-dependent half on every hit.
    let (class, msg) = raised(send_value_dyn_cached(
        &SITE,
        0,
        &recv,
        secret,
        &[],
        None,
        stranger.0,
    ));
    assert_eq!(class, zeo_abi::NO_METHOD_ERROR_CLASS);
    assert!(msg.starts_with("private method 'secret'"), "{msg}");
}
