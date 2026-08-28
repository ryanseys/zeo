//! Class relations and frame labels: `is_a`/`is_a_value` and the rescue
//! matchers, `module_cmp`, subclass/ancestor enumeration, the Ractor
//! moved-value probes, and the synthetic C-frame labels (`NOFRAME`/
//! `SPECIALIZED`/`c_frame_label`).

use super::*;

/// Builtin rows that push NO synthetic frame: rows whose CRuby counterpart is
/// frameless in a backtrace (`send`, `raise`), and rows whose implementation
/// reads the CALLER's frames and would see its own synthetic one instead
/// (`caller`, `binding`, `__method__`, `warn`'s uplevel). The eval family
/// stays frameless too: its frames carry `(eval)` locations built inside
/// the eval entries, not a dispatch-boundary label.
const NOFRAME: &[&str] = &[
    // `Proc#call`/`Method#call` are frameless in CRuby backtraces -- the
    // callee's own frame sits directly on the caller's.
    "call",
    "caller",
    "caller_locations",
    "each_caller_location",
    "raise",
    "fail",
    "throw",
    "binding",
    "local_variables",
    "__method__",
    "__callee__",
    "block_given?",
    "iterator?",
    "warn",
    "puts",
    "print",
    "p",
    "pp",
    "putc",
    "send",
    "__send__",
    "public_send",
    "eval",
    "instance_eval",
    "instance_exec",
    "class_eval",
    "class_exec",
    "module_eval",
    "module_exec",
];

/// Rows CRuby answers with a SPECIALIZED VM instruction rather than a call:
/// `opt_ltlt`, `opt_aset` and `opt_aref` push no control frame, so a raise
/// from inside one names only the caller (`"x".freeze << "y"` reports
/// `<main>`, where a hand-written `def <<` would report itself).
///
/// Keyed by (class, name), because the specialization is per receiver type and
/// the general fallback DOES frame: `opt_ltlt` covers String and Array and
/// nothing else, so `IO#<<` reports `IO#write` / `IO#<<` and must not be
/// silenced by a name-only rule. `opt_aref` covers Array and Hash alone, so
/// `String#[]` keeps its frame -- oracle-verified in both directions
/// (`[1,2][2**70]` reports `<main>`, `"ab"[2**70]` reports `String#[]`).
///
/// The cost of a per-ROW table where CRuby decides per SITE: `a.send(:[], i)`
/// is a real call in CRuby and frames, and here it does not. That is the same
/// trade the `<<` and `[]=` rows already make, and it errs toward the
/// spelling that is written.
const SPECIALIZED: &[(ClassId, &str)] = &[
    // `Foo.new` pushes NO frame in ruby: an ordinary class, a Struct
    // subclass and an exception subclass all report the CALLER directly.
    // Two shapes do frame, and each pushes its own rather than relying on
    // this row -- `Class.new { }` MINTING a class (`Class#initialize` +
    // `Class#new`), and a Data class, whose `new` is a distinct cfunc and
    // reports as `D.new`. Both are oracle-verified.
    //
    // Suppressed here rather than at the call because the rule is about the
    // RECEIVER (`Class` itself vs an ordinary class) and this table is keyed
    // by the row's owner, which is `Class` in both cases.
    (zeo_abi::CLASS_CLASS, "new"),
    (zeo_abi::STRING_CLASS, "<<"),
    (zeo_abi::ARRAY_CLASS, "<<"),
    (zeo_abi::ARRAY_CLASS, "[]"),
    (zeo_abi::ARRAY_CLASS, "[]="),
    (zeo_abi::HASH_CLASS, "[]"),
    (zeo_abi::HASH_CLASS, "[]="),
    // The TOTAL query rows (CRuby's opt_length / opt_size / opt_empty_p /
    // opt_succ): no argument, no coercion, no raising shape at all -- a
    // husk receiver raises in DISPATCH, before any frame -- so the frame
    // was pure per-call cost and its absence is unobservable. The
    // per-row-vs-per-site trade above does not bite here: there is no
    // raising fallback shape to mis-report. String#+/==, Symbol#==, and
    // Array#max/min stay framed: their coercing/raising shapes frame in
    // CRuby (and opt_newarray_send is per-SITE, literals only).
    (zeo_abi::ARRAY_CLASS, "length"),
    (zeo_abi::ARRAY_CLASS, "size"),
    (zeo_abi::ARRAY_CLASS, "empty?"),
    (zeo_abi::STRING_CLASS, "length"),
    (zeo_abi::STRING_CLASS, "size"),
    (zeo_abi::STRING_CLASS, "empty?"),
    (zeo_abi::HASH_CLASS, "length"),
    (zeo_abi::HASH_CLASS, "size"),
    (zeo_abi::HASH_CLASS, "empty?"),
    (zeo_abi::INTEGER_CLASS, "succ"),
    (zeo_abi::STRING_CLASS, "succ"),
];

/// The interned backtrace label for a builtin row -- `'Owner#name'` for an
/// instance row, `'Owner.name'` for a class row -- or `None` for [`NOFRAME`]
/// and [`SPECIALIZED`].
/// One leak per distinct row, cached, so the cold MRO-walk paths can ask on
/// every call; the flat maps precompute it into [`FlatHit`] instead.
pub(crate) fn c_frame_label(owner: ClassId, name: Symbol, sep: char) -> Option<&'static str> {
    let n = name.name_str();
    if NOFRAME.contains(&n) {
        return None;
    }
    if sep == '#' && SPECIALIZED.contains(&(owner, n)) {
        return None;
    }
    type LabelKey = (u32, Symbol, bool);
    static LABELS: std::sync::Mutex<Option<crate::FMap<LabelKey, &'static str>>> =
        std::sync::Mutex::new(None);
    let key = (owner.0, name, sep == '#');
    let mut cache = LABELS.lock().expect("label cache");
    let map = cache.get_or_insert_with(crate::FMap::default);
    if let Some(&label) = map.get(&key) {
        return Some(label);
    }
    let label: &'static str = Box::leak(format!("{}{sep}{n}", class_name(owner)?).into_boxed_str());
    map.insert(key, label);
    Some(label)
}

/// Run `f` under the row's synthetic C frame, or bare for a `None` label,
/// and turn a key-projection exception parked under it back into an error
/// (see `collections::take_key_raise` -- a user `hash` has no return channel
/// inside the projection, so the row it ran under reports it).
#[inline]
pub(crate) fn with_c_frame(
    label: Option<&'static str>,
    f: impl FnOnce() -> Result<RubyValue, Signal>,
) -> Result<RubyValue, Signal> {
    let r = match label {
        Some(l) => {
            let _frame = crate::frames::synthetic_c_frame(l);
            f()
        }
        None => f(),
    };
    crate::value::collections::check_key_raise(r)
}

/// [`with_c_frame`] straight from the row's ids: the frame carries
/// (owner, name, separator) verbatim and the label materializes only if
/// something READS it -- the cold MRO-walk paths asked
/// [`c_frame_label`]'s mutex-backed intern on every call for a string
/// that a no-raise call never looks at. The [`NOFRAME`]/[`SPECIALIZED`]
/// verdicts (frameless rows) are the same sets the label path consults.
#[inline]
pub(crate) fn with_c_frame_ids(
    owner: ClassId,
    name: Symbol,
    sep: char,
    f: impl FnOnce() -> Result<RubyValue, Signal>,
) -> Result<RubyValue, Signal> {
    let n = name.name_str();
    let frameless = NOFRAME.contains(&n) || (sep == '#' && SPECIALIZED.contains(&(owner, n)));
    let r = if frameless {
        f()
    } else {
        let _frame = crate::frames::synthetic_c_frame_ids(owner, name, sep == '.');
        f()
    };
    crate::value::collections::check_key_raise(r)
}

/// Every registered class that has `id` in its ancestry, `id` itself excluded
/// -- the descendants a change to `id` can be seen through. One scan of the
/// registry, called only from `runtime_meta::patch_class` on a real runtime
/// definition, never from a loop.
///
/// Reads the FROZEN ancestry deliberately: a class whose chain was spliced at
/// runtime is covered by `GATE_ANCESTRY_MUTATED` instead, which is a stronger
/// statement than anything this could enumerate.
pub(crate) fn classes_with_ancestor(id: ClassId) -> Vec<u32> {
    let Some(reg) = REGISTRY.get() else {
        return Vec::new();
    };
    reg.entries
        .iter()
        .filter(|(_, e)| e.ancestors.contains(&id))
        .map(|(cid, _)| cid)
        .collect()
}

/// Every registered class id, or every module id -- `ObjectSpace.each_object`
/// asked for `Class` or `Module`.
///
/// This walk is COMPLETE and needs no allocation registry: a class is not an
/// `Arc` a program allocates, it is a row registered once at startup (plus
/// whatever `Class.new` minted since). CRuby's `Module` answer includes
/// classes, since every Class is a Module.
pub fn class_ids(modules_too: bool) -> Vec<ClassId> {
    let Some(reg) = REGISTRY.get() else {
        return Vec::new();
    };
    let mut ids: Vec<ClassId> = reg
        .entries
        .iter()
        .filter(|(_, e)| modules_too || !e.is_module)
        .map(|(cid, _)| ClassId(cid))
        .collect();
    // Dense iteration is already id-ordered; the sort keeps the printed
    // order an explicit contract rather than a storage accident.
    ids.sort_unstable_by_key(|c| c.0);
    ids
}

/// `recv_class.is_a?(target)` -- a real ancestry check against the SAME
/// linearized `ancestors` list `super`/reflection uses at compile time (see
/// `analyze::mro::compute_ancestors`'s docs), not zeo's own two-tier
/// "transplant for dispatch, shallower list for reflection" split (confirmed
/// to diverge on a module-of-module diamond -- see the plan). The one
/// runtime surface this needs for `Signal::Raise`/`rescue` matching:
/// there's no first-class `Class`/`Module` runtime VALUE (can't be stored in
/// a variable or reflected on generally), just this narrow "is this concrete
/// class id ancestor-compatible with that one" check.
pub fn is_a(recv_class: ClassId, target: ClassId) -> bool {
    ancestors_of_value(recv_class).contains(&target)
}

/// The modules `cid`'s class body `extend`ed, or empty for a class that
/// extended nothing (and for one born at runtime -- `Class.new { extend M }`
/// records into `runtime_meta`'s map instead).
pub(crate) fn class_extends(cid: ClassId) -> &'static [ClassId] {
    REGISTRY
        .get()
        .and_then(|r| r.entries.get(&cid.0))
        .map_or(&[], |e| &e.extends)
}

/// `is_a?` over a VALUE rather than a class id -- the ancestry check plus the
/// modules `obj.extend(M)` mixed into this one object. A class id cannot
/// express those: `extend` leaves the class alone and files the module on the
/// receiver's singleton, so two objects of the same class disagree.
///
/// Every observable `is_a?` (`Kernel#is_a?`, `Module#===`, `case`/`when`, a
/// `ClassCheck` pattern) goes through here. `is_a` itself stays for the
/// internal checks that hold a class id and nothing else.
pub fn is_a_value(recv: &RubyValue, target: ClassId) -> bool {
    // A moved container answers as the husk class it now is (`class_id`
    // cannot say so for `Str`/`Array`/`Hash`, which carry no class word) --
    // `crate::value::observed_class_id` is the same probe for the callers
    // that want the id itself. Gated on the process-wide moved gate, so a
    // program that never moves pays one shared-byte load.
    if crate::runtime_meta::any_moved() && value_moved(recv) {
        return is_a(zeo_abi::RACTOR_MOVED_OBJECT_CLASS, target);
    }
    is_a(recv.class_id(), target)
        || crate::runtime_meta::value_extends(recv, target)
        || is_a_singleton_class(recv, target)
}

/// Whether `v` is a husk a `Ractor` move left behind: a gutted container
/// (its `Freezable` flags byte says so) or a retagged object. Callers gate
/// on [`crate::runtime_meta::any_moved`] first, so the flag probes only run
/// once a move has actually happened.
pub(crate) fn value_moved(v: &RubyValue) -> bool {
    match v {
        RubyValue::Str(s) => s.is_moved(),
        RubyValue::Array(a) => a.is_moved(),
        RubyValue::Hash(h) => h.is_moved(),
        RubyValue::Object(o) => o.class_id() == zeo_abi::RACTOR_MOVED_OBJECT_CLASS,
        _ => false,
    }
}

/// Codegen's guard ahead of an inlined accessor field access (emitted only
/// for a program that names `Ractor` -- the emission switch): a Path-1
/// receiver bypasses dispatch, so the husk check runs here instead.
pub fn check_not_moved_obj<T: RubyObject>(o: &T) -> Result<(), Signal> {
    if o.class_id() == zeo_abi::RACTOR_MOVED_OBJECT_CLASS {
        return Err(crate::ractor::moved_object_error());
    }
    Ok(())
}

/// `Foo.is_a?(Foo.singleton_class)` -- true in CRuby, because `CLASS_OF(Foo)`
/// IS that singleton class. zeo mints singleton classes on demand instead of
/// threading the parallel `#<Class:Sub> < #<Class:Foo>` chain, so the relation
/// is answered from the id's OWNER: a class instantiates its own singleton
/// class and every ancestor's, and any other value only its own.
fn is_a_singleton_class(recv: &RubyValue, target: ClassId) -> bool {
    if !crate::runtime_meta::is_live() {
        return false;
    }
    match crate::runtime_meta::singleton_owner_value(target) {
        Some(RubyValue::Class(owner)) => {
            matches!(recv, RubyValue::Class(cid) if is_a(*cid, owner))
        }
        // An object's singleton class has exactly one instance, so this is
        // identity, not equality.
        Some(owner) => {
            let key = crate::runtime_meta::value_identity(&owner);
            key.is_some() && key == crate::runtime_meta::value_identity(recv)
        }
        None => false,
    }
}

/// `rescue *list => e` matching: does the raised `exc` match any class in the
/// splatted `list`?
///
/// `list` is the EVALUATED splat expression, and the splat's own rule decides
/// what it becomes -- the same rule an argument list follows:
///
///   * an Array is its elements (`rescue *errs`);
///   * `nil` is NO elements, so the clause matches nothing and the exception
///     goes on past it;
///   * anything else with a `to_a` is that array;
///   * anything else is one element (`rescue *ArgumentError`).
///
/// A non-Module element is then CRuby's `TypeError: class or module required
/// for rescue clause`. OR'd in after a clause's static class list by codegen.
///
/// THE NIL CASE IS NOT AN EDGE CASE. rubygems writes
///
/// ```ruby
/// rescue Gem::Timeout::Error, IOError, SocketError, SystemCallError,
///        *(OpenSSL::SSL::SSLError if Gem::HAVE_OPENSSL) => e
/// ```
///
/// and without openssl loaded that splat is nil. Treating it as one element
/// raised `TypeError` in place of whatever the body raised -- but only for an
/// exception the earlier entries did NOT match, which is why it hid until a
/// download failed. Measured against ruby 4.0.6 for all six shapes; see
/// `tests/a_rescue_splat_follows_the_splat_rule.rb`.
pub fn rescue_matches_any(exc: &RubyValue, list: &RubyValue) -> Result<bool, Signal> {
    match list {
        RubyValue::Array(a) => {
            for el in a.lock().to_vec() {
                if rescue_class_matches(&el, exc)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        RubyValue::Nil => Ok(false),
        // A Class is the common single-element splat and has no `to_a`, so it
        // skips the probe -- this path runs with an exception in flight.
        RubyValue::Class(_) => rescue_class_matches(list, exc),
        single => {
            let to_a = Symbol::intern("to_a");
            if crate::dispatch::responds_to_value(single, to_a, true) {
                let expanded = send_value(single, to_a, &[], None)?;
                // One level only: `to_a` answering something that is not an
                // Array is the caller's problem, and the element check names
                // it.
                if let RubyValue::Array(a) = expanded {
                    for el in a.lock().to_vec() {
                        if rescue_class_matches(&el, exc)? {
                            return Ok(true);
                        }
                    }
                    return Ok(false);
                }
            }
            rescue_class_matches(single, exc)
        }
    }
}

fn rescue_class_matches(cls: &RubyValue, exc: &RubyValue) -> Result<bool, Signal> {
    match cls {
        RubyValue::Class(cid) => {
            // `rescue` matches via `===` (CRuby's rule): `Module#===` is the
            // ancestry test, taken directly -- unless the matcher OVERRIDES
            // `self.===`, which is how rspec-support's
            // `AllExceptionsExceptOnesWeMustNotRescue` catches everything but
            // NoMemory/SignalException. The override is rare, the probe cheap,
            // and this path runs only while an exception is in flight.
            if class_method_owner(*cid, Symbol::intern("===")).is_some() {
                let verdict =
                    send_value(cls, Symbol::intern("==="), std::slice::from_ref(exc), None)?;
                return Ok(verdict.truthy());
            }
            Ok(is_a(exc.as_object_unchecked().class_id(), *cid))
        }
        _ => Err(crate::builtins::type_error!(
            "class or module required for rescue clause"
        )),
    }
}

/// `Module#<=>`-style ordering of two class/module ids by the ancestry
/// relation (CRuby `rb_class_cmp`): `Less` when `a` is a proper descendant of
/// `b`, `Greater` when a proper ancestor, `Equal` when the same class, and
/// `None` when the two are unrelated (neither appears in the other's
/// linearized ancestors). Backs `Module`'s `<`/`<=`/`>`/`>=`/`<=>` rows.
pub fn module_cmp(a: ClassId, b: ClassId) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let a_le_b = is_a(a, b); // b in a's ancestors: a is b-or-below
    let b_le_a = is_a(b, a);
    match (a_le_b, b_le_a) {
        (true, true) => Some(Ordering::Equal),
        (true, false) => Some(Ordering::Less),
        (false, true) => Some(Ordering::Greater),
        (false, false) => None,
    }
}

/// The DIRECT subclasses of `cid` (CRuby `Class#subclasses`): every registered
/// non-module class whose immediate superclass -- the first non-module entry
/// after itself in its linearized ancestry -- is `cid`. Order is unspecified
/// in CRuby, so callers that need determinism sort by name.
pub fn direct_subclasses(cid: ClassId) -> Vec<ClassId> {
    let Some(r) = REGISTRY.get() else {
        return Vec::new();
    };
    r.entries
        .iter()
        .filter(|(_, e)| !e.is_module)
        .filter_map(|(id, e)| {
            let self_id = ClassId(id);
            if self_id == cid {
                return None;
            }
            let sup = e
                .ancestors
                .iter()
                .skip_while(|&&a| a != self_id)
                .skip(1)
                .find(|&&a| !class_is_module(a).unwrap_or(false))?;
            (*sup == cid).then_some(self_id)
        })
        .collect()
}
