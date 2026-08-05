//! Native `Struct`/`Data` -- the CRuby-faithful RUNTIME model, the single path
//! for `Struct.new`/`Data.define` in every position.
//!
//! `Struct.new(:a, :b)` and `Data.define(:a, :b)` MINT A REAL CLASS at runtime
//! (an overlay class id rooted at `STRUCT_CLASS`/`DATA_CLASS`), so a struct is a
//! first-class value usable anywhere -- assigned to a local, used inline, or as
//! a superclass -- not only on a constant's right-hand side.
//!
//! One native `StructInstance` RObj backs every value, distinguished by its
//! `class_id` (the same "one native type, many class ids" shape
//! `value_subclass`/`exception` use). Its payload is an ordered `slots` vector;
//! the member NAMES live once per struct class in `STRUCT_META`. The shared
//! behaviour (`==`/`each`/`to_a`/`[]`/`to_h`/`inspect`/...) lives ONCE in
//! `STRUCT_CLASS`/`DATA_CLASS`'s `class_table`, reached through `send_in`'s MRO
//! walk -- never monomorphized per struct. Per-member accessors (`p.x`, `p.x=`)
//! are the only per-class methods: native `MethodImpl::Dynamic` closures that
//! index a captured slot, installed on the minted class's overlay entry.
//!
//! DIVERGENCE: because a struct class is a RUNTIME value rather than a
//! compile-time class, it cannot be a STATIC superclass. A two-step
//! `Point = Struct.new(:x, :y); class Foo < Point` fails to compile
//! (`unknown superclass Point`). The commoner inline idiom
//! `class Foo < Struct.new(...)` was never
//! supported anyway (a superclass expression isn't statically resolvable), and
//! runtime struct subclasses (`class Bar < baz` for a runtime `baz`) inherit
//! members correctly via `meta_of`'s ancestor walk. Full compile-time
//! subclassing of a struct constant would need runtime class definition with a
//! dynamic superclass -- a separate feature.

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

use parking_lot::Mutex;
use zeo_abi::{ClassId, STRUCT_CLASS};

use crate::builtins::{
    arg_error, block_or_enum, index_error, inherited_row, name_error, type_error,
};
use crate::dispatch::{MethodImpl, RObj, RubyObject, class_name, raise_error, send_in, send_value};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use crate::{array_new, hash_new, string_new};
use zeo_macros::ruby_class;

// ---------------------------------------------------------------------------
// Per-class metadata: the member list, shared by every instance of a struct
// class (keyed by the class id, walked through ancestors so a subclass of a
// struct inherits it).
// ---------------------------------------------------------------------------

pub struct StructMeta {
    pub members: Vec<Symbol>,
    /// `Data.define` -> immutable (frozen, readers only, no `each`/`[]=`).
    pub is_data: bool,
    /// `Struct.new(..., keyword_init: true)` -- construct by keyword only.
    /// `None` when never specified (so `keyword_init?` answers `nil`, as CRuby
    /// does), `Some(true)`/`Some(false)` when the option was passed explicitly.
    pub keyword_init: Option<bool>,
}

impl StructMeta {
    pub(crate) fn index_of(&self, sym: Symbol) -> Option<usize> {
        self.members.iter().position(|&m| m == sym)
    }
}

static STRUCT_META: LazyLock<RwLock<HashMap<u32, Arc<StructMeta>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn put_meta(id: ClassId, meta: StructMeta) {
    STRUCT_META.write().unwrap().insert(id.0, Arc::new(meta));
}

/// Register a class whose members zeo read off a LITERAL `Struct.new(:a, :b)`
/// and compiled to a real generated class, rather than one minted at runtime.
/// Called once per such class from generated `main()`.
///
/// Nothing else changes: the class inherits `STRUCT_CLASS`/`DATA_CLASS`'s
/// `class_table`, and every row in there reaches members BY INDEX
/// ([`slots_of`]/[`slot_get`]/[`slot_set`]), which both representations answer.
pub fn register_compiled_struct(
    id: ClassId,
    members: &[&str],
    is_data: bool,
    keyword_init: Option<bool>,
) {
    put_meta(
        id,
        StructMeta {
            members: members.iter().map(|m| Symbol::intern(m)).collect(),
            is_data,
            keyword_init,
        },
    );
}

/// The struct/data metadata for `class_id` -- its own, or the nearest struct
/// ancestor's (so `class Foo < PointStruct` inherits `Point`'s members).
pub fn meta_of(class_id: ClassId) -> Option<Arc<StructMeta>> {
    let table = STRUCT_META.read().unwrap();
    if let Some(m) = table.get(&class_id.0) {
        return Some(m.clone());
    }
    for anc in crate::dispatch::ancestors_of_value(class_id) {
        if let Some(m) = table.get(&anc.0) {
            return Some(m.clone());
        }
    }
    None
}

/// Whether `class_id` is (or descends from) a native struct/data class -- gates
/// the struct class-method hook in `dispatch::send_value_in`.
pub fn is_struct_class(class_id: ClassId) -> bool {
    meta_of(class_id).is_some()
}

/// Marshal's `S` dump payload: each member symbol paired with the receiver's
/// slot value, in declaration order. `None` when `recv` is not a struct
/// instance (so the caller falls through to the plain-object path).
pub fn marshal_members(recv: &RubyValue) -> Option<Vec<(Symbol, RubyValue)>> {
    let RubyValue::Object(o) = recv else {
        return None;
    };
    let meta = meta_of(o.class_id())?;
    Some(meta.members.iter().copied().zip(slots_of(recv)).collect())
}

/// The parameter shape of a struct/data member accessor `name` on `class_id`,
/// for `Method#arity`/`#parameters` (these accessors are dispatched dynamically,
/// so they register no descriptor of their own). A reader (`:x`) takes no args; a
/// writer (`:x=`, structs only) takes one required `value`. `None` when `name`
/// is not a member accessor of this class.
pub fn accessor_params(class_id: ClassId, name: Symbol) -> Option<crate::method_meta::Descriptor> {
    use crate::method_meta::ParamKind;
    let meta = meta_of(class_id)?;
    let n = name.name();
    if let Some(base) = n.strip_suffix('=') {
        if !meta.is_data && meta.members.iter().any(|m| m.name() == base) {
            return Some(vec![(ParamKind::Req, Some("value".to_string()))]);
        }
        return None;
    }
    meta.members.iter().any(|m| m.name() == n).then(Vec::new)
}

/// If `recv` is a Struct/Data instance with a member literally named `name`,
/// that member's current slot value. Lets a member named after a Kernel
/// universal (`Struct.new(:class)` / `Data.define(:hash)`) shadow the builtin
/// when the universal is otherwise served by a codegen fast path that never
/// reaches the accessor.
pub fn member_value_named(recv: &RObj, name: &str) -> Option<RubyValue> {
    let meta = meta_of(recv.class_id())?;
    let i = meta.index_of(Symbol::intern(name))?;
    Some(slot_get(&RubyValue::Object(recv.clone()), i))
}

// ---------------------------------------------------------------------------
// The instance
// ---------------------------------------------------------------------------

pub struct StructInstance {
    class_id: ClassId,
    frozen: AtomicBool,
    slots: Mutex<Vec<RubyValue>>,
    ivars: Mutex<Vec<(String, RubyValue)>>,
}

impl StructInstance {
    pub(crate) fn new_robj(class_id: ClassId, slots: Vec<RubyValue>) -> RObj {
        Arc::new(StructInstance {
            class_id,
            frozen: AtomicBool::new(false),
            slots: Mutex::new(slots),
            ivars: Mutex::new(Vec::new()),
        })
    }
}

impl RubyObject for StructInstance {
    fn class_id(&self) -> ClassId {
        self.class_id
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        self.ivars.lock().iter().map(|(_, v)| v.clone()).collect()
    }
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        self.ivars
            .lock()
            .iter()
            .map(|(k, v)| (format!("@{k}"), v.clone()))
            .collect()
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        self.ivars
            .lock()
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        let mut ivars = self.ivars.lock();
        match ivars.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = v,
            None => ivars.push((name.to_string(), v)),
        }
        true
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        // The plain rule only: Data's copies-stay-frozen guarantee (#2716)
        // is applied AFTER the copy hooks run ([`refreeze_data_copy`] from
        // `Kernel#dup`/`#clone`), CRuby's ordering -- the hook must see the
        // pre-freeze copy.
        Arc::new(StructInstance {
            class_id: self.class_id,
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
            slots: Mutex::new(self.slots.lock().clone()),
            ivars: Mutex::new(self.ivars.lock().clone()),
        })
    }
}

/// A `Data` instance stays frozen through EVERY copy -- dup and
/// `clone(freeze: false)` alike (#2716). Called by the `Kernel` copy rows
/// after the hooks ran; a non-Data value is untouched.
pub(crate) fn refreeze_data_copy(v: &RubyValue) {
    if let RubyValue::Object(o) = v
        && meta_of(o.class_id()).is_some_and(|m| m.is_data)
    {
        o.set_frozen();
    }
}

/// The receiver's class id, whichever representation backs it.
pub(crate) fn recv_class_id(recv: &RubyValue) -> ClassId {
    match recv {
        RubyValue::Object(o) => o.class_id(),
        _ => unreachable!("struct table row on a non-object receiver"),
    }
}

/// Every member, in declaration order, SNAPSHOT -- so nothing downstream runs
/// while a slot lock is held.
///
/// Two representations answer here, and every row in this module works on both
/// because it only ever asks for members BY INDEX, which is the one thing they
/// have in common. A runtime `Struct.new` mints an overlay class whose
/// instances are [`StructInstance`], an ordered slot vector. One zeo could read
/// off a literal member list is an ordinary generated class whose members are
/// HIDDEN slots on its own Rust struct (`ruby_class!`'s `hidden` block),
/// reached through `RubyObject::hidden_ivar_get`.
pub(crate) fn slots_of(recv: &RubyValue) -> Vec<RubyValue> {
    let RubyValue::Object(o) = recv else {
        unreachable!("struct table row on a non-object receiver")
    };
    if let Some(inst) = o.as_any().downcast_ref::<StructInstance>() {
        return inst.slots.lock().clone();
    }
    let n = meta_of(o.class_id()).map_or(0, |m| m.members.len());
    (0..n)
        .map(|i| o.hidden_ivar_get(i).unwrap_or(RubyValue::Nil))
        .collect()
}

/// One member by index -- [`slots_of`] when more than one is wanted.
pub(crate) fn slot_get(recv: &RubyValue, i: usize) -> RubyValue {
    let RubyValue::Object(o) = recv else {
        unreachable!("struct table row on a non-object receiver")
    };
    if let Some(inst) = o.as_any().downcast_ref::<StructInstance>() {
        return inst.slots.lock()[i].clone();
    }
    o.hidden_ivar_get(i).unwrap_or(RubyValue::Nil)
}

pub(crate) fn slot_set(recv: &RubyValue, i: usize, v: RubyValue) {
    let RubyValue::Object(o) = recv else {
        unreachable!("struct table row on a non-object receiver")
    };
    if let Some(inst) = o.as_any().downcast_ref::<StructInstance>() {
        inst.slots.lock()[i] = v;
        return;
    }
    o.hidden_ivar_set(i, v);
}

/// The member-symbol array shared by `Struct#members`/`Data#members`.
pub(crate) fn build_members(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let meta = meta_of(recv_class_id(recv)).expect("struct instance has meta");
    Ok(RubyValue::Array(array_new(
        meta.members.iter().map(|m| RubyValue::Symbol(*m)).collect(),
    )))
}

// ---------------------------------------------------------------------------
// Shared helpers used by the method rows
// ---------------------------------------------------------------------------

/// Resolve `key` (Integer index, Symbol, or String) to a member slot index --
/// the shared core of `[]`/`[]=`/`dig`. `IndexError`/`NameError` on a miss,
/// matching CRuby.
fn member_index(recv: &RubyValue, key: &RubyValue) -> Result<usize, Signal> {
    let meta = meta_of(recv_class_id(recv)).expect("struct instance has meta");
    let n = meta.members.len();
    match key {
        RubyValue::Symbol(s) => meta
            .index_of(*s)
            .ok_or_else(|| name_error!("no member '{}' in struct", s.name())),
        RubyValue::Str(s) => {
            let name = s.lock().to_utf8_lossy().into_owned();
            meta.index_of(Symbol::intern(&name))
                .ok_or_else(|| name_error!("no member '{name}' in struct"))
        }
        other => {
            let i = crate::builtins::convert::to_index(other)?;
            let idx = if i < 0 { i + n as i64 } else { i };
            if idx < 0 || idx as usize >= n {
                return Err(index_error!("offset {i} too large for struct(size:{n})"));
            }
            Ok(idx as usize)
        }
    }
}

/// A member value rendered with its own `inspect` (dispatched, so a member that
/// is itself a struct/object with a custom `inspect` renders correctly).
fn inspect_slot(v: &RubyValue) -> Result<String, Signal> {
    let s = send_value(v, Symbol::intern("inspect"), &[], None)?;
    Ok(match s {
        RubyValue::Str(buf) => buf.lock().to_utf8_lossy().into_owned(),
        other => other.inspect_string(),
    })
}

fn build_to_h(recv: &RubyValue) -> RubyValue {
    let meta = meta_of(recv_class_id(recv)).expect("struct instance has meta");
    let slots = slots_of(recv);
    let pairs: Vec<(RubyValue, RubyValue)> = meta
        .members
        .iter()
        .zip(slots.iter())
        .map(|(m, v)| (RubyValue::Symbol(*m), v.clone()))
        .collect();
    RubyValue::Hash(hash_new(pairs))
}

/// `Struct#to_h`/`Data#to_h`: the plain member-keyed Hash, or -- with a block --
/// a Hash built from the `[key, value]` pairs the block returns for each
/// `(member_sym, value)` pair (CRuby's `rb_struct_to_h` block form).
pub(crate) fn struct_to_h(recv: &RubyValue, block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = block else {
        return Ok(build_to_h(recv));
    };
    let meta = meta_of(recv_class_id(recv)).expect("struct instance has meta");
    let slots = slots_of(recv);
    let mut pairs = Vec::with_capacity(meta.members.len());
    for (m, v) in meta.members.iter().zip(slots.iter()) {
        let ret = p.call(&[RubyValue::Symbol(*m), v.clone()])?;
        let RubyValue::Array(a) = &ret else {
            return Err(type_error!(
                "wrong element type {} (expected array)",
                crate::builtins::class_name_of(&ret)
            ));
        };
        let kv = a.lock();
        if kv.len() != 2 {
            return Err(arg_error!(
                "element has wrong array length (expected 2, was {})",
                kv.len()
            ));
        }
        pairs.push((kv[0].clone(), kv[1].clone()));
    }
    Ok(RubyValue::Hash(hash_new(pairs)))
}

pub(crate) fn build_inspect(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let meta = meta_of(recv_class_id(recv)).expect("struct instance has meta");
    let slots = slots_of(recv);
    let mut parts = Vec::with_capacity(meta.members.len());
    for (m, v) in meta.members.iter().zip(slots.iter()) {
        let label = crate::builtins::symbol::struct_member_label(&m.name());
        parts.push(format!("{label}={}", inspect_slot(v)?));
    }
    let kind = if meta.is_data { "data" } else { "struct" };
    // An anonymous struct/data shows no name (`#<struct x=1>`); a named one
    // shows it (`#<struct Point x=1>`), matching CRuby. An unnamed runtime
    // class reports a `#<Class:0x..>` placeholder from `class_name` -- treat
    // that as anonymous.
    let named = match class_name(recv_class_id(recv)) {
        Some(n) if !n.is_empty() && !n.starts_with("#<Class:") => format!(" {n}"),
        _ => String::new(),
    };
    // The separator goes BETWEEN the pieces, never ahead of the list: a
    // memberless Data is `#<data Empty>`, not `#<data Empty >`.
    let body = if parts.is_empty() {
        String::new()
    } else {
        format!(" {}", parts.join(", "))
    };
    Ok(RubyValue::Str(string_new(format!(
        "#<{kind}{named}{body}>"
    ))))
}

pub(crate) fn struct_equal(recv: &RubyValue, other: &RubyValue) -> bool {
    let RubyValue::Object(o) = other else {
        return false;
    };
    if meta_of(o.class_id()).is_none() || recv_class_id(recv) != o.class_id() {
        return false;
    }
    // Each side snapshot separately rather than both guards held at once:
    // `s == s` aliases the SAME instance, so locking the second while the
    // first is still held would deadlock a non-reentrant `Mutex`. Comparing
    // the cloned values keeps a NaN member making `s == s` false, as CRuby.
    let a = slots_of(recv);
    let b = slots_of(other);
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.rb_eq(y))
}

pub(crate) fn deconstruct_keys(recv: &RubyValue, keys: &RubyValue) -> Result<RubyValue, Signal> {
    if matches!(keys, RubyValue::Nil) {
        return Ok(build_to_h(recv));
    }
    let meta = meta_of(recv_class_id(recv)).expect("struct instance has meta");
    let RubyValue::Array(arr) = keys else {
        return Ok(RubyValue::Hash(hash_new(Vec::new())));
    };
    let keys = arr.lock().clone();
    if keys.len() > meta.members.len() {
        return Ok(RubyValue::Hash(hash_new(Vec::new())));
    }
    let slots = slots_of(recv);
    let mut out: Vec<(RubyValue, RubyValue)> = Vec::new();
    for k in keys {
        match &k {
            RubyValue::Symbol(s) => match meta.index_of(*s) {
                Some(i) => out.push((k.clone(), slots[i].clone())),
                None => break, // CRuby returns what it has so far on a miss
            },
            _ => break,
        }
    }
    Ok(RubyValue::Hash(hash_new(out)))
}

// ---------------------------------------------------------------------------
// STRUCT_CLASS instance methods (the mutable, Enumerable kind)
// ---------------------------------------------------------------------------

ruby_class! {
    Struct = zeo_abi::STRUCT_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    // `Struct.new(:a, :b)` / `Struct.new("Name", :a, :b, keyword_init: true)`
    // MINTS a real subclass at runtime. No `[]` twin: `Struct[:a]` is a
    // NoMethodError in CRuby, because `[]` is an alias of `new` installed on
    // each MINTED subclass (`class_lookup` below), not on `Struct` itself.
    def self."new" (_recv, *args, &block) {
        define_value_class(STRUCT_CLASS, false, args, block)
    }

    def "initialize"(recv, *args, &_block) {
        bind_members(recv, args, false)?;
        Ok(RubyValue::Nil)
    }
    // `initialize_copy` -- the slot vector and nothing else, and only
    // between instances of the SAME struct class (CRuby's
    // "initialize_copy should take same class object").
    private def "initialize_copy"(recv, other) {
        if recv.is_frozen() {
            return Err(frozen_error(recv));
        }
        // Class-ID equality, not a payload downcast: a compile-time
        // `S = Struct.new(...)` synthesizes a GENERATED struct class whose
        // instances are not `StructInstance` -- the id is the honest test.
        let same_class = other.class_id() == recv.class_id() && meta_of(recv.class_id()).is_some();
        if !same_class {
            return Err(crate::builtins::type_error!(
                "initialize_copy should take same class object"
            ));
        }
        let values = slots_of(other);
        for (i, v) in values.into_iter().enumerate() {
            slot_set(recv, i, v);
        }
        Ok(recv.clone())
    }
    def "members"(recv) {
        build_members(recv)
    }
    def "to_a" | "values" | "deconstruct" (recv) {
        Ok(RubyValue::Array(array_new(slots_of(recv))))
    }
    def "to_h"(recv, &block) {
        struct_to_h(recv, block)
    }
    def "each"(recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        for v in slots_of(recv) {
            p.call(&[v])?;
        }
        Ok(recv.clone())
    }
    def "each_pair"(recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        let meta = meta_of(recv_class_id(recv)).expect("struct instance has meta");
        let slots = slots_of(recv);
        for (m, v) in meta.members.iter().zip(slots.iter()) {
            p.call(&[RubyValue::Symbol(*m), v.clone()])?;
        }
        Ok(recv.clone())
    }
    def "[]"(recv, arg) {
        let i = member_index(recv, arg)?;
        Ok(slot_get(recv, i))
    }
    def "[]="(recv, arg1, arg2) {
        if recv.is_frozen() {
            return Err(frozen_error(recv));
        }
        let i = member_index(recv, arg1)?;
        slot_set(recv, i, (*arg2).clone());
        Ok((*arg2).clone())
    }
    def "values_at"(recv, *args, &block) {
        let arr = RubyValue::Array(array_new(slots_of(recv)));
        send_value(&arr, Symbol::intern("values_at"), args, block)
    }
    def "dig" cfunc (recv, _key, *_rest, &_block) {
        let args = __args;
        let value = {
            let i = member_index(recv, &args[0])?;
            slot_get(recv, i)
        };
        if args.len() == 1 || matches!(value, RubyValue::Nil) {
            return Ok(value);
        }
        send_value(&value, Symbol::intern("dig"), &args[1..], None)
    }
    def "size" | "length" (recv) {
        let meta = meta_of(recv_class_id(recv)).expect("struct instance has meta");
        Ok(RubyValue::Int(meta.members.len() as i64))
    }
    def "=="(recv, other) {
        Ok(RubyValue::Bool(struct_equal(recv, other)))
    }
    def "eql?"(recv, arg) {
        Ok(RubyValue::Bool(struct_equal(recv, arg)))
    }
    def "hash"(recv) {
        let arr = RubyValue::Array(array_new(slots_of(recv)));
        send_value(&arr, Symbol::intern("hash"), &[], None)
    }
    def "deconstruct_keys"(recv, arg) {
        deconstruct_keys(recv, arg)
    }
    def "inspect" | "to_s" (recv) {
        build_inspect(recv)
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "select" cfunc (recv, *_args, &block) { inherited_row!(enumerable, "select", recv, __args, block) }
    def "filter" cfunc (recv, *_args, &block) { inherited_row!(enumerable, "filter", recv, __args, block) }
}

pub(crate) fn frozen_error(recv: &RubyValue) -> Signal {
    let name = class_name(recv.class_id()).unwrap_or_else(|| "Struct".to_string());
    crate::dispatch::raise_error_details(
        "FrozenError",
        format!("can't modify frozen {name}: {}", recv.inspect_string()),
        &[("receiver", recv.clone())],
    )
}

/// Leading positional arguments with a trailing all-symbol-keyed Hash --
/// `P.new(1, y: 2)`. Refused rather than treated as a short form, since the
/// keywords route to a keyword initializer that takes no positionals.
fn mixes_positional_and_keywords(args: &[RubyValue]) -> bool {
    if args.len() < 2 {
        return false;
    }
    let Some(RubyValue::Hash(h)) = args.last() else {
        return false;
    };
    let g = h.lock();
    !g.is_empty() && g.values().all(|(k, _)| matches!(k, RubyValue::Symbol(_)))
}

/// The default member-setter shared by `Struct#initialize`/`Data#initialize`:
/// bind constructor args to slots. Positional (nil-filling for a plain Struct,
/// exact-arity for keyword_init/Data) or by keyword (a trailing Hash).
pub(crate) fn bind_members(
    recv: &RubyValue,
    args: &[RubyValue],
    is_data: bool,
) -> Result<(), Signal> {
    let meta = meta_of(recv_class_id(recv)).expect("struct instance has meta");
    let n = meta.members.len();

    // Keyword construction: a plain Struct when declared `keyword_init:`, OR
    // when the caller WROTE keywords (the kw-marked hash -- CRuby decides by
    // `rb_keyword_given_p`, not the declaration; `keyword_init: false` still
    // forces positional). Data when the sole arg is a keyword hash.
    let kw_hash = match args.last() {
        Some(RubyValue::Hash(h)) if args.len() == 1 => {
            let by_declaration = meta.keyword_init == Some(true) || is_data;
            let by_call_shape =
                meta.keyword_init.is_none() && crate::collections::hash_is_kwargs(h);
            (by_declaration || by_call_shape).then_some(h)
        }
        _ => None,
    };

    if let Some(h) = kw_hash {
        let mut seen = vec![false; n];
        // Pairs snapshot before any slot is written: holding the hash's own
        // lock across a write that could reach back into it is the hazard the
        // rest of this module already avoids the same way.
        let pairs: Vec<(RubyValue, RubyValue)> = h.lock().values().cloned().collect();
        for (k, v) in pairs {
            let RubyValue::Symbol(s) = k else {
                return Err(arg_error!("keyword must be a symbol"));
            };
            match meta.index_of(s) {
                Some(i) => {
                    slot_set(recv, i, v.clone());
                    seen[i] = true;
                }
                None => {
                    return Err(arg_error!("unknown keyword: :{}", s.name()));
                }
            }
        }
        // Data requires every member; a plain keyword_init Struct nil-fills.
        if is_data {
            let missing: Vec<String> = seen
                .iter()
                .enumerate()
                .filter(|(_, s)| !**s)
                .map(|(i, _)| format!(":{}", meta.members[i].name()))
                .collect();
            if !missing.is_empty() {
                let word = if missing.len() == 1 {
                    "keyword"
                } else {
                    "keywords"
                };
                return Err(arg_error!("missing {word}: {}", missing.join(", ")));
            }
        }
        return Ok(());
    }

    // Mixed positional + keyword (`P.new(1, y: 2)`): a trailing symbol-keyed
    // Hash alongside leading positional args is an illegal mix for Data and
    // keyword_init Structs -- CRuby routes keywords to the keyword initializer,
    // which accepts no positional args, so it reports "given N, expected 0".
    if (is_data || meta.keyword_init == Some(true)) && mixes_positional_and_keywords(args) {
        return Err(arg_error!(
            "wrong number of arguments (given {}, expected 0)",
            args.len()
        ));
    }

    // Positional. A plain Struct nil-fills a short arg list; keyword_init and
    // Data require exact arity.
    if meta.keyword_init == Some(true) && !args.is_empty() {
        return Err(arg_error!(
            "wrong number of arguments (given {}, expected 0)",
            args.len()
        ));
    }
    if args.len() > n || (is_data && args.len() != n && !args.is_empty()) {
        return Err(raise_error(
            "ArgumentError",
            if is_data {
                format!(
                    "wrong number of arguments (given {}, expected {})",
                    args.len(),
                    n
                )
            } else {
                "struct size differs".to_string()
            },
        ));
    }
    if is_data && args.is_empty() && n > 0 {
        let missing: Vec<String> = meta
            .members
            .iter()
            .map(|m| format!(":{}", m.name()))
            .collect();
        let word = if missing.len() == 1 {
            "keyword"
        } else {
            "keywords"
        };
        return Err(arg_error!("missing {word}: {}", missing.join(", ")));
    }
    for (i, a) in args.iter().enumerate() {
        slot_set(recv, i, a.clone());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Construction: `SomeStruct.new(...)` -> a StructInstance whose `initialize`
// (the default member-setter, or a user override) binds the slots.
// ---------------------------------------------------------------------------

pub fn struct_construct(
    class_id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let meta = meta_of(class_id).expect("struct construct on a class with no meta");
    // `Data`'s two constructor forms are the SAME constructor: ruby zips
    // positional arguments against the member list in `new` (`rb_data_s_new`),
    // so `initialize` -- the default one below or a user override -- only ever
    // sees keywords, and a SHORTFALL is reported as a missing keyword rather
    // than as an arity error. Refusing an incomplete instance is the whole
    // reason Data exists; Struct, its mutable half, really does allow the
    // short form and keeps its own message.
    let zipped;
    let args = if meta.is_data && !matches!(args, [RubyValue::Hash(_)]) {
        // Positionals AND keywords together is not a short form -- it is an
        // illegal mix, and must be refused before the zip below turns the
        // trailing hash into a member's value.
        if mixes_positional_and_keywords(args) {
            return Err(arg_error!(
                "wrong number of arguments (given {}, expected 0)",
                args.len()
            ));
        }
        crate::builtins::check_arity(args.len(), 0, Some(meta.members.len()))?;
        zipped = [RubyValue::Hash(hash_new(
            meta.members
                .iter()
                .zip(args)
                .map(|(m, v)| (RubyValue::Symbol(*m), v.clone()))
                .collect(),
        ))];
        &zipped[..]
    } else {
        args
    };
    // A COMPILED struct is an ordinary generated class: allocate through its
    // own registered allocator, so the instance is the Rust struct its
    // accessors and its `initialize` were compiled against. Building a
    // `StructInstance` for it instead compiles fine and then reads every
    // member as nil.
    let handle = crate::dispatch::allocate_instance_of(class_id).unwrap_or_else(|| {
        StructInstance::new_robj(class_id, vec![RubyValue::Nil; meta.members.len()])
    });
    // Dispatch `initialize` so a user override (and its `super`) resolve
    // normally; the default member-setter lives in the class_table.
    send_in(0, &handle, Symbol::intern("initialize"), args, block)?;
    if meta.is_data {
        handle.set_frozen();
    }
    Ok(RubyValue::Object(handle))
}

// ---------------------------------------------------------------------------
// Class creation: `Struct.new(...)` / `Data.define(...)`.
// ---------------------------------------------------------------------------

/// [`parse_members`]'s result: the optional leading class-name string, the
/// member symbols, and the `keyword_init:` flag if one was given.
type ParsedMembers = (Option<String>, Vec<Symbol>, Option<bool>);

/// Parse the member symbols (and, for `Struct`, an optional leading string
/// name and a trailing `keyword_init:` hash) from the class-creation args.
fn parse_members(args: &[RubyValue], is_data: bool) -> Result<ParsedMembers, Signal> {
    let mut rest = args;
    let mut name = None;
    let mut keyword_init = None;

    // `Struct.new("Name", :a, :b)` -- an optional leading String class name.
    if !is_data {
        if let Some(RubyValue::Str(s)) = rest.first() {
            name = Some(s.lock().to_utf8_lossy().into_owned());
            rest = &rest[1..];
        }
        // `Struct.new(:a, :b, keyword_init: true)` -- a trailing options hash.
        if let Some(RubyValue::Hash(h)) = rest.last() {
            for (k, v) in h.lock().values() {
                if let RubyValue::Symbol(s) = k
                    && s.name() == "keyword_init"
                {
                    keyword_init = Some(v.truthy());
                    continue;
                }
                return Err(arg_error!("unknown keyword"));
            }
            rest = &rest[..rest.len() - 1];
        }
    }

    let mut members: Vec<Symbol> = Vec::with_capacity(rest.len());
    for a in rest {
        let sym = match a {
            RubyValue::Symbol(s) => *s,
            RubyValue::Str(s) => Symbol::intern(&s.lock().to_utf8_lossy()),
            other => {
                return Err(type_error!(
                    "{} is not a symbol nor a string",
                    other.inspect_string()
                ));
            }
        };
        if members.contains(&sym) {
            return Err(arg_error!("duplicate member: {}", sym.name()));
        }
        members.push(sym);
    }
    Ok((name, members, keyword_init))
}

/// Mint the class: allocate a runtime id rooted at `STRUCT_CLASS`/`DATA_CLASS`,
/// register its metadata + per-member accessor methods, run any class-body
/// block, and return the class value.
pub(crate) fn define_value_class(
    root: ClassId,
    is_data: bool,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let (name, members, keyword_init) = parse_members(args, is_data)?;

    // Per-member accessor closures: a reader for every member, plus a writer
    // for a mutable Struct. Each captures its slot index.
    let mut methods: crate::FMap<Symbol, MethodImpl> = crate::FMap::default();
    for (i, &m) in members.iter().enumerate() {
        methods.insert(
            m,
            MethodImpl::Dynamic(Arc::new(move |recv: &RObj, _a: &[RubyValue], _b| {
                let inst = recv
                    .as_any()
                    .downcast_ref::<StructInstance>()
                    .expect("accessor on a non-struct");
                Ok(inst.slots.lock()[i].clone())
            })),
        );
        if !is_data {
            methods.insert(
                Symbol::intern(&format!("{}=", m.name())),
                MethodImpl::Dynamic(Arc::new(move |recv: &RObj, a: &[RubyValue], _b| {
                    let inst = recv
                        .as_any()
                        .downcast_ref::<StructInstance>()
                        .expect("writer on a non-struct");
                    if inst.is_frozen() {
                        return Err(frozen_error(&RubyValue::Object(recv.clone())));
                    }
                    inst.slots.lock()[i] = a[0].clone();
                    Ok(a[0].clone())
                })),
            );
        }
    }

    // `inspect`/`to_s`/`hash` are defined on `Object`/`Kernel` in the FROZEN
    // REGISTRY, which the runtime-class ancestor walk (`walk_runtime_class`)
    // consults per ancestor BEFORE `send_in`'s `class_table` MRO walk ever
    // reaches `STRUCT_CLASS`. So a struct's own versions (which live in
    // `class_table`) would be shadowed by `Object#inspect` et al. Install them
    // as overlay deltas on THIS class id -- the first ancestor walked -- so
    // they win, forwarding to the one shared `class_table` implementation.
    // Struct's rows live in `builtins::rstruct`, Data's in `builtins::data`;
    // both self-register via linkme, so reach the root's instance table by id
    // rather than naming the sibling module's generated `lookup`.
    let root_table = crate::builtins::registered_table(root)
        .and_then(|t| t.instance.as_ref())
        .expect("Struct/Data root has a registered instance table");
    // `==`/`eql?` are installed as overlays too so the low-level
    // `call_user_method` fallback (`RubyValue::rb_eq`, and thus `Array#==`/
    // `#include?`/`#index`/`Hash#==`) reaches the struct's VALUE comparison
    // instead of `Object#==`'s reference identity -- the class_table row alone
    // isn't consulted by that path (only registry/overlay methods are).
    for shadowed in ["inspect", "to_s", "hash", "==", "eql?"] {
        // A member named after one of these (`Struct.new(:hash)` /
        // `Data.define(:hash)`) keeps its accessor: the member reader must
        // shadow the struct's builtin, matching CRuby.
        if members.contains(&Symbol::intern(shadowed)) {
            continue;
        }
        if let Some(f) = (root_table.lookup)(shadowed) {
            methods.insert(
                Symbol::intern(shadowed),
                MethodImpl::Dynamic(Arc::new(move |recv: &RObj, a: &[RubyValue], b| {
                    f(&RubyValue::Object(recv.clone()), a, b)
                })),
            );
        }
    }

    let class_id = crate::runtime_meta::intern_native_class(root, methods, struct_construct);
    put_meta(
        class_id,
        StructMeta {
            members,
            is_data,
            keyword_init,
        },
    );
    if let Some(n) = &name {
        crate::runtime_meta::name_runtime_class_if_anonymous(class_id, n);
    }

    let class_val = RubyValue::Class(class_id);
    // A class-body block (`Struct.new(:x) do def dist; ...; end end`) runs with
    // `self` bound to the new class, so its `def`s register on it.
    if let Some(RubyValue::Proc(p)) = &block {
        p.call_with_self(&class_val, &[])?;
    }
    Ok(class_val)
}

// ---------------------------------------------------------------------------
// Class methods on a MINTED struct/data class (`Point.members`, `Point[1,2]`).
// Probed by `dispatch::send_value_in` for a `RubyValue::Class` receiver that
// `is_struct_class`, since these are not inherited through the Class ancestry.
// ---------------------------------------------------------------------------

pub fn class_lookup(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    match name {
        "members" => Some(class_members),
        "new" | "[]" => Some(class_new_instance),
        "keyword_init?" => Some(class_keyword_init),
        _ => None,
    }
}

fn class_members(
    recv: &RubyValue,
    _args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let RubyValue::Class(cid) = recv else {
        unreachable!("struct class method on a non-class receiver")
    };
    let meta = meta_of(*cid).expect("struct class has meta");
    Ok(RubyValue::Array(array_new(
        meta.members.iter().map(|m| RubyValue::Symbol(*m)).collect(),
    )))
}

fn class_new_instance(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let RubyValue::Class(cid) = recv else {
        unreachable!("struct class method on a non-class receiver")
    };
    struct_construct(*cid, args, block)
}

fn class_keyword_init(
    recv: &RubyValue,
    _args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let RubyValue::Class(cid) = recv else {
        unreachable!("struct class method on a non-class receiver")
    };
    let meta = meta_of(*cid).expect("struct class has meta");
    // Tri-state, matching CRuby: `nil` when `keyword_init:` was never given,
    // otherwise the boolean it was set to.
    Ok(match meta.keyword_init {
        Some(b) => RubyValue::Bool(b),
        None => RubyValue::Nil,
    })
}
