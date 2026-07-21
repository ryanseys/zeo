//! Native `Struct`/`Data` (Batch E) -- the CRuby-faithful RUNTIME model, the
//! single path for `Struct.new`/`Data.define` in every position (the old
//! compile-time source-text synthesis for the constant form has been retired).
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
//! DIVERGENCE (the cost of the flip): because a struct class is a RUNTIME value
//! rather than a compile-time class, it cannot be a STATIC superclass. A
//! two-step `Point = Struct.new(:x, :y); class Foo < Point` fails to compile
//! (`unknown superclass Point`), where the old compile-time synthesis allowed
//! it. The commoner inline idiom `class Foo < Struct.new(...)` was never
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
use zeo_abi::{ClassId, DATA_CLASS, STRUCT_CLASS};

use crate::builtins::{
    arg_error, arity, block_or_enum, builtin_methods, frozen_error, index_error, name_error,
    type_error,
};
use crate::dispatch::{MethodImpl, RObj, RubyObject, class_name, raise_error, send_in, send_value};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use crate::{array_new, hash_new, string_new};

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
    fn index_of(&self, sym: Symbol) -> Option<usize> {
        self.members.iter().position(|&m| m == sym)
    }
}

static STRUCT_META: LazyLock<RwLock<HashMap<u32, Arc<StructMeta>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn put_meta(id: ClassId, meta: StructMeta) {
    STRUCT_META.write().unwrap().insert(id.0, Arc::new(meta));
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

/// The parameter shape of a struct/data member accessor `name` on `class_id`,
/// for `Method#arity`/`#parameters` (these accessors are dispatched dynamically,
/// so no `register_params` descriptor exists). A reader (`:x`) takes no args; a
/// writer (`:x=`, structs only) takes one required `value`. `None` when `name`
/// is not a member accessor of this class.
pub fn accessor_params(
    class_id: ClassId,
    name: Symbol,
) -> Option<crate::method_params::Descriptor> {
    use crate::method_params::ParamKind;
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
    fn new_robj(class_id: ClassId, slots: Vec<RubyValue>) -> RObj {
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
        Arc::new(StructInstance {
            class_id: self.class_id,
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
            slots: Mutex::new(self.slots.lock().clone()),
            ivars: Mutex::new(self.ivars.lock().clone()),
        })
    }
}

/// Downcast a struct-table receiver -- the `class_table` keying guarantees it.
fn recv_struct(recv: &RubyValue) -> &StructInstance {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<StructInstance>()
            .expect("struct table row on a non-struct receiver"),
        _ => unreachable!("struct table row on a non-object receiver"),
    }
}

fn slots_of(recv: &RubyValue) -> Vec<RubyValue> {
    recv_struct(recv).slots.lock().clone()
}

// ---------------------------------------------------------------------------
// Shared helpers used by the method rows
// ---------------------------------------------------------------------------

/// Resolve `key` (Integer index, Symbol, or String) to a member slot index --
/// the shared core of `[]`/`[]=`/`dig`. `IndexError`/`NameError` on a miss,
/// matching CRuby.
fn member_index(recv: &RubyValue, key: &RubyValue) -> Result<usize, Signal> {
    let inst = recv_struct(recv);
    let meta = meta_of(inst.class_id).expect("struct instance has meta");
    let n = meta.members.len();
    match key {
        RubyValue::Int(i) => {
            let idx = if *i < 0 { *i + n as i64 } else { *i };
            if idx < 0 || idx as usize >= n {
                return Err(index_error!("offset {i} too large for struct(size:{n})"));
            }
            Ok(idx as usize)
        }
        RubyValue::Symbol(s) => meta
            .index_of(*s)
            .ok_or_else(|| name_error!("no member '{}' in struct", s.name())),
        RubyValue::Str(s) => {
            let name = s.lock().to_utf8_lossy().into_owned();
            meta.index_of(Symbol::intern(&name))
                .ok_or_else(|| name_error!("no member '{name}' in struct"))
        }
        other => Err(type_error!(
            "no implicit conversion of {} into Integer",
            class_name(other.class_id()).unwrap_or_default()
        )),
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
    let inst = recv_struct(recv);
    let meta = meta_of(inst.class_id).expect("struct instance has meta");
    let slots = inst.slots.lock();
    let pairs: Vec<(RubyValue, RubyValue)> = meta
        .members
        .iter()
        .zip(slots.iter())
        .map(|(m, v)| (RubyValue::Symbol(*m), v.clone()))
        .collect();
    RubyValue::Hash(hash_new(pairs))
}

fn build_inspect(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let inst = recv_struct(recv);
    let meta = meta_of(inst.class_id).expect("struct instance has meta");
    let slots = inst.slots.lock().clone();
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
    let named = match class_name(inst.class_id) {
        Some(n) if !n.is_empty() && !n.starts_with("#<Class:") => format!("{n} "),
        _ => String::new(),
    };
    Ok(RubyValue::Str(string_new(format!(
        "#<{kind} {named}{}>",
        parts.join(", ")
    ))))
}

fn struct_equal(recv: &RubyValue, other: &RubyValue) -> bool {
    let me = recv_struct(recv);
    let RubyValue::Object(o) = other else {
        return false;
    };
    let Some(them) = o.as_any().downcast_ref::<StructInstance>() else {
        return false;
    };
    if me.class_id != them.class_id {
        return false;
    }
    // Clone each slot vec (releasing its lock at the end of the statement)
    // rather than hold both guards at once: `s == s` aliases the SAME
    // `StructInstance`, so locking `them.slots` while `me.slots` is still
    // held would deadlock (parking_lot's Mutex is not reentrant). Comparing
    // the cloned values keeps a NaN member making `s == s` false, as CRuby.
    let a = me.slots.lock().clone();
    let b = them.slots.lock().clone();
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.rb_eq(y))
}

fn deconstruct_keys(recv: &RubyValue, keys: &RubyValue) -> Result<RubyValue, Signal> {
    if matches!(keys, RubyValue::Nil) {
        return Ok(build_to_h(recv));
    }
    let inst = recv_struct(recv);
    let meta = meta_of(inst.class_id).expect("struct instance has meta");
    let RubyValue::Array(arr) = keys else {
        return Ok(RubyValue::Hash(hash_new(Vec::new())));
    };
    let keys = arr.lock().clone();
    if keys.len() > meta.members.len() {
        return Ok(RubyValue::Hash(hash_new(Vec::new())));
    }
    let slots = inst.slots.lock();
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

builtin_methods! {
    pub(crate) fn lookup;

    "initialize" => fn struct_initialize(recv, args, _block) {
        bind_members(recv, args, false)?;
        Ok(RubyValue::Nil)
    }

    "members" => fn members(recv, _args, _block) {
        let inst = recv_struct(recv);
        let meta = meta_of(inst.class_id).expect("struct instance has meta");
        Ok(RubyValue::Array(array_new(meta.members.iter().map(|m| RubyValue::Symbol(*m)).collect())))
    }

    "to_a" | "values" | "deconstruct" => fn to_a(recv, _args, _block) {
        Ok(RubyValue::Array(array_new(slots_of(recv))))
    }

    "to_h" => fn to_h(recv, _args, _block) {
        Ok(build_to_h(recv))
    }

    "each" => fn each(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each", args, block);
        for v in slots_of(recv) {
            p.call(&[v])?;
        }
        Ok(recv.clone())
    }

    "each_pair" => fn each_pair(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_pair", args, block);
        let inst = recv_struct(recv);
        let meta = meta_of(inst.class_id).expect("struct instance has meta");
        let slots = slots_of(recv);
        for (m, v) in meta.members.iter().zip(slots.iter()) {
            p.call(&[RubyValue::Symbol(*m), v.clone()])?;
        }
        Ok(recv.clone())
    }

    "[]" => fn index(recv, args, _block) {
        arity!(args, 1);
        let i = member_index(recv, &args[0])?;
        Ok(recv_struct(recv).slots.lock()[i].clone())
    }

    "[]=" => fn index_set(recv, args, _block) {
        arity!(args, 2);
        let inst = recv_struct(recv);
        if inst.is_frozen() {
            return Err(frozen_error(recv));
        }
        let i = member_index(recv, &args[0])?;
        inst.slots.lock()[i] = args[1].clone();
        Ok(args[1].clone())
    }

    "values_at" => fn values_at(recv, args, block) {
        let arr = RubyValue::Array(array_new(slots_of(recv)));
        send_value(&arr, Symbol::intern("values_at"), args, block)
    }

    "dig" => fn dig(recv, args, _block) {
        if args.is_empty() {
            return Err(arg_error!("wrong number of arguments (given 0, expected 1+)"));
        }
        let value = {
            let i = member_index(recv, &args[0])?;
            recv_struct(recv).slots.lock()[i].clone()
        };
        if args.len() == 1 || matches!(value, RubyValue::Nil) {
            return Ok(value);
        }
        send_value(&value, Symbol::intern("dig"), &args[1..], None)
    }

    "size" | "length" => fn size(recv, _args, _block) {
        let inst = recv_struct(recv);
        let meta = meta_of(inst.class_id).expect("struct instance has meta");
        Ok(RubyValue::Int(meta.members.len() as i64))
    }

    "==" => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(struct_equal(recv, &args[0])))
    }

    "eql?" => fn eql(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(struct_equal(recv, &args[0])))
    }

    "hash" => fn hash(recv, _args, _block) {
        let arr = RubyValue::Array(array_new(slots_of(recv)));
        send_value(&arr, Symbol::intern("hash"), &[], None)
    }

    "deconstruct_keys" => fn deconstruct_keys_m(recv, args, _block) {
        arity!(args, 1);
        deconstruct_keys(recv, &args[0])
    }

    "inspect" | "to_s" => fn inspect(recv, _args, _block) {
        build_inspect(recv)
    }
}

// ---------------------------------------------------------------------------
// DATA_CLASS instance methods (immutable, no Enumerable / `[]` / writers)
// ---------------------------------------------------------------------------

builtin_methods! {
    pub(crate) fn lookup_data;

    "initialize" => fn data_initialize(recv, args, _block) {
        bind_members(recv, args, true)?;
        Ok(RubyValue::Nil)
    }

    "members" => fn data_members(recv, _args, _block) {
        members(recv, &[], None)
    }

    "to_h" => fn data_to_h(recv, _args, _block) {
        Ok(build_to_h(recv))
    }

    "deconstruct" => fn data_deconstruct(recv, _args, _block) {
        Ok(RubyValue::Array(array_new(slots_of(recv))))
    }

    "deconstruct_keys" => fn data_deconstruct_keys(recv, args, _block) {
        arity!(args, 1);
        deconstruct_keys(recv, &args[0])
    }

    "with" => fn data_with(recv, args, _block) {
        // `d.with(x: 1)` -- a copy with the named members replaced. Changes
        // arrive as a trailing keyword hash (the G2 convention).
        let inst = recv_struct(recv);
        let meta = meta_of(inst.class_id).expect("data instance has meta");
        let mut slots = inst.slots.lock().clone();
        if let Some(RubyValue::Hash(h)) = args.last() {
            for (k, v) in h.lock().values() {
                let RubyValue::Symbol(s) = k else {
                    return Err(arg_error!("unknown keyword"));
                };
                match meta.index_of(*s) {
                    Some(i) => slots[i] = v.clone(),
                    None => {
                        return Err(arg_error!("unknown keyword: :{}", s.name()))
                    }
                }
            }
        }
        let copy = StructInstance::new_robj(inst.class_id, slots);
        copy.set_frozen();
        Ok(RubyValue::Object(copy))
    }

    "==" => fn data_eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(struct_equal(recv, &args[0])))
    }

    "eql?" => fn data_eql(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(struct_equal(recv, &args[0])))
    }

    "hash" => fn data_hash(recv, _args, _block) {
        let h = build_to_h(recv);
        send_value(&h, Symbol::intern("hash"), &[], None)
    }

    "inspect" | "to_s" => fn data_inspect(recv, _args, _block) {
        build_inspect(recv)
    }
}

fn frozen_error(recv: &RubyValue) -> Signal {
    let name = class_name(recv.class_id()).unwrap_or_else(|| "Struct".to_string());
    frozen_error!("can't modify frozen {name}")
}

/// The default member-setter shared by `Struct#initialize`/`Data#initialize`:
/// bind constructor args to slots. Positional (nil-filling for a plain Struct,
/// exact-arity for keyword_init/Data) or by keyword (a trailing Hash).
fn bind_members(recv: &RubyValue, args: &[RubyValue], is_data: bool) -> Result<(), Signal> {
    let inst = recv_struct(recv);
    let meta = meta_of(inst.class_id).expect("struct instance has meta");
    let n = meta.members.len();

    // Keyword construction: a plain Struct only when declared `keyword_init:`;
    // Data when the sole arg is a keyword hash (else positional).
    let kw_hash = match args.last() {
        Some(RubyValue::Hash(h))
            if (meta.keyword_init == Some(true) || is_data) && args.len() == 1 =>
        {
            Some(h)
        }
        _ => None,
    };

    if let Some(h) = kw_hash {
        let mut slots = inst.slots.lock();
        let mut seen = vec![false; n];
        for (k, v) in h.lock().values() {
            let RubyValue::Symbol(s) = k else {
                return Err(arg_error!("keyword must be a symbol"));
            };
            match meta.index_of(*s) {
                Some(i) => {
                    slots[i] = v.clone();
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
    let mut slots = inst.slots.lock();
    for (i, a) in args.iter().enumerate() {
        slots[i] = a.clone();
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
    let handle = StructInstance::new_robj(class_id, vec![RubyValue::Nil; meta.members.len()]);
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
                if let RubyValue::Symbol(s) = k {
                    if s.name() == "keyword_init" {
                        keyword_init = Some(v.truthy());
                        continue;
                    }
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
fn define_value_class(
    root: ClassId,
    is_data: bool,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let (name, members, keyword_init) = parse_members(args, is_data)?;

    // Per-member accessor closures: a reader for every member, plus a writer
    // for a mutable Struct. Each captures its slot index.
    let mut methods: HashMap<Symbol, MethodImpl> = HashMap::new();
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
    let table: fn(&str) -> Option<crate::builtins::BuiltinMethodFn> =
        if is_data { lookup_data } else { lookup };
    for shadowed in ["inspect", "to_s", "hash"] {
        if let Some(f) = table(shadowed) {
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

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" | "[]" => fn struct_new(_recv, args, block) {
        define_value_class(STRUCT_CLASS, false, args, block)
    }
}

builtin_methods! {
    pub(crate) fn lookup_class_data;

    "define" => fn data_define(_recv, args, block) {
        define_value_class(DATA_CLASS, true, args, block)
    }
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
