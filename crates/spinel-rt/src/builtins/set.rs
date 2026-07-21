//! `Set` (CRuby's `set.rb`, now a core autoloaded class) -- a runtime-resident
//! `RObj` whose membership is backed by an `RHash` (element -> unit), so it
//! reuses the exact `HashKey` projection Hash keys use: insertion order,
//! `eql?`/`hash` semantics, and cross-encoding string rules all for free.
//! `Enumerable` is mixed in via the ABI (`SET_CLASS`'s `includes`), so every
//! iteration method (`map`/`select`/`count`/...) drives the `each` row below;
//! only the set-specific surface lives here.

use crate::builtins::{arity, block_or_enum, builtin_methods};
use crate::dispatch::{raise_error, RObj, RubyObject};
use crate::{RHash, RubyValue, Signal};
use spinel_abi::SET_CLASS;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct RSet {
    /// Element -> unit. The value is unused; membership is key presence, and
    /// `hash_keys` recovers the original element values in insertion order.
    hash: RHash,
    frozen: AtomicBool,
}

impl RSet {
    /// The elements in insertion order.
    fn elements(&self) -> Vec<RubyValue> {
        self.hash.lock().values().map(|(k, _)| k.clone()).collect()
    }

    fn len(&self) -> usize {
        self.hash.lock().len()
    }

    fn contains(&self, v: &RubyValue) -> bool {
        crate::hash_has_key(&self.hash, v)
    }

    /// Adds `v`, returning whether it was newly inserted (false if already
    /// present) -- the shared core of `add`/`<<`/`add?`.
    fn insert(&self, v: RubyValue) -> bool {
        if self.contains(&v) {
            return false;
        }
        crate::hash_set(&self.hash, v, RubyValue::Bool(true));
        true
    }

    /// Drops every element and re-seeds from `elements` (deduplicated by the
    /// insert path) -- the in-place core of `replace`/`map!`/`flatten!` and
    /// the `select!`/`reject!` family.
    fn replace_contents(&self, elements: impl IntoIterator<Item = RubyValue>) {
        self.hash.lock().clear();
        for e in elements {
            self.insert(e);
        }
    }
}

impl RubyObject for RSet {
    fn class_id(&self) -> crate::ClassId {
        SET_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let dup = RSet { hash: crate::hash_new(vec![]), frozen: AtomicBool::new(copy_frozen) };
        for e in self.elements() {
            dup.insert(e);
        }
        Arc::new(dup)
    }
}

/// A fresh empty Set value.
fn empty_set() -> RubyValue {
    RubyValue::Object(Arc::new(RSet { hash: crate::hash_new(vec![]), frozen: AtomicBool::new(false) }))
}

/// A Set value seeded with `elements` (deduplicated by the insert path).
pub(crate) fn set_from(elements: impl IntoIterator<Item = RubyValue>) -> RubyValue {
    let s = RSet { hash: crate::hash_new(vec![]), frozen: AtomicBool::new(false) };
    for e in elements {
        s.insert(e);
    }
    RubyValue::Object(Arc::new(s))
}

/// The `RSet` behind a Set receiver -- the row only dispatches on one.
fn set_of(recv: &RubyValue) -> &RSet {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RSet>()
            .expect("the Set table only dispatches on Set receivers"),
        _ => unreachable!("the Set table only dispatches on Set receivers"),
    }
}

/// The elements of any enumerable argument (`Set`/`Array`/anything with
/// `to_a`), for the set-algebra operators and `merge`.
fn arg_elements(v: &RubyValue) -> Result<Vec<RubyValue>, Signal> {
    match v {
        RubyValue::Object(o) if o.class_id() == SET_CLASS => Ok(set_of(v).elements()),
        RubyValue::Array(a) => Ok(a.lock().iter().cloned().collect()),
        other => {
            // CRuby's `do_with_enum` requires the source to respond to `each`;
            // a non-enumerable (an Integer, ...) is an ArgumentError, not the
            // NoMethodError a bare `to_a` send would surface.
            if !crate::dispatch::responds_to_value(other, crate::Symbol::intern("each"), false) {
                return Err(raise_error(
                    "ArgumentError",
                    "value must be enumerable".to_string(),
                ));
            }
            let arr = crate::dispatch::send_value(other, crate::Symbol::intern("to_a"), &[], None)?;
            match arr {
                RubyValue::Array(a) => Ok(a.lock().iter().cloned().collect()),
                _ => Err(raise_error(
                    "ArgumentError",
                    "value must be enumerable".to_string(),
                )),
            }
        }
    }
}

/// Raises `FrozenError` when the receiver is frozen -- the guard every
/// mutating row runs first (CRuby's `rb_check_frozen`).
fn check_frozen(recv: &RubyValue) -> Result<(), Signal> {
    if set_of(recv).is_frozen() {
        return Err(raise_error(
            "FrozenError",
            "can't modify frozen Set".to_string(),
        ));
    }
    Ok(())
}

/// Whether `v` is itself a Set -- the recursion test for `flatten`.
fn is_set(v: &RubyValue) -> bool {
    matches!(v, RubyValue::Object(o) if o.class_id() == SET_CLASS)
}

/// Appends `s`'s elements to `out`, recursively expanding any nested Set --
/// the shared core of `flatten`/`flatten!`.
fn flatten_into(s: &RSet, out: &mut Vec<RubyValue>) {
    for e in s.elements() {
        if is_set(&e) {
            flatten_into(set_of(&e), out);
        } else {
            out.push(e);
        }
    }
}

/// Keeps the elements for which `block`'s truthiness equals `keep_truthy`
/// (so `select!`/`keep_if` pass `true`, `reject!`/`delete_if` pass `false`),
/// rewriting the receiver in place. Returns whether anything was removed --
/// the `!`-variants answer nil on no change.
fn filter_in_place(recv: &RubyValue, block: &crate::RProc, keep_truthy: bool) -> Result<bool, Signal> {
    let s = set_of(recv);
    let before = s.elements();
    let mut kept = Vec::with_capacity(before.len());
    for e in &before {
        if block.call(std::slice::from_ref(e))?.truthy() == keep_truthy {
            kept.push(e.clone());
        }
    }
    let changed = kept.len() != before.len();
    s.replace_contents(kept);
    Ok(changed)
}

/// Groups the receiver's elements by `block`'s return value into a
/// `Hash{ key => Set }`, keys in first-seen order (CRuby `Set#classify`).
/// `divide`'s single-arg form is just the values of this map.
fn classify_groups(recv: &RubyValue, block: &crate::RProc) -> Result<RHash, Signal> {
    let groups = crate::hash_new(vec![]);
    for e in set_of(recv).elements() {
        let key = block.call(std::slice::from_ref(&e))?;
        if !crate::hash_has_key(&groups, &key) {
            crate::hash_set(&groups, key.clone(), empty_set());
        }
        set_of(&crate::hash_get(&groups, &key)).insert(e);
    }
    Ok(groups)
}

/// Tarjan's strongly-connected components over the directed graph `adj`
/// (`adj[u]` = the nodes `u` points at). Backs `Set#divide`'s two-arg form,
/// where an edge `u -> v` exists iff `block.call(u, v)` is truthy.
fn strongly_connected_components(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut index = vec![usize::MAX; n]; // discovery order, MAX = unvisited
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next = 0usize;
    let mut out: Vec<Vec<usize>> = Vec::new();

    // Iterative DFS -- an explicit frame stack of (node, next-child-cursor)
    // avoids blowing the Rust stack on a large adjacency.
    for start in 0..n {
        if index[start] != usize::MAX {
            continue;
        }
        let mut frames: Vec<(usize, usize)> = vec![(start, 0)];
        index[start] = next;
        low[start] = next;
        next += 1;
        stack.push(start);
        on_stack[start] = true;

        while let Some(&(u, cursor)) = frames.last() {
            if cursor < adj[u].len() {
                frames.last_mut().unwrap().1 += 1;
                let v = adj[u][cursor];
                if index[v] == usize::MAX {
                    index[v] = next;
                    low[v] = next;
                    next += 1;
                    stack.push(v);
                    on_stack[v] = true;
                    frames.push((v, 0));
                } else if on_stack[v] {
                    low[u] = low[u].min(index[v]);
                }
            } else {
                if low[u] == index[u] {
                    let mut component = Vec::new();
                    loop {
                        let w = stack.pop().unwrap();
                        on_stack[w] = false;
                        component.push(w);
                        if w == u {
                            break;
                        }
                    }
                    out.push(component);
                }
                frames.pop();
                if let Some(&(parent, _)) = frames.last() {
                    low[parent] = low[parent].min(low[u]);
                }
            }
        }
    }
    out
}

builtin_methods! {
    pub(crate) fn lookup;

    "add"[1] | "<<"[1] => fn add(recv, args, _block) {
        arity!(args, 1);
        check_frozen(recv)?;
        set_of(recv).insert(args[0].clone());
        Ok(recv.clone())
    }
    // `add?`: nil if the element was already present, else self (post-add).
    "add?"[1] => fn add_p(recv, args, _block) {
        arity!(args, 1);
        check_frozen(recv)?;
        if set_of(recv).insert(args[0].clone()) {
            Ok(recv.clone())
        } else {
            Ok(RubyValue::Nil)
        }
    }
    "include?"[1] | "member?"[1] | "==="[1] | "contain?" => fn include_p(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(set_of(recv).contains(&args[0])))
    }
    "delete"[1] => fn delete(recv, args, _block) {
        arity!(args, 1);
        check_frozen(recv)?;
        crate::hash_delete(&set_of(recv).hash, &args[0]);
        Ok(recv.clone())
    }
    // `delete?`: self if the element was present and removed, else nil.
    "delete?"[1] => fn delete_p(recv, args, _block) {
        arity!(args, 1);
        check_frozen(recv)?;
        let s = set_of(recv);
        if s.contains(&args[0]) {
            crate::hash_delete(&s.hash, &args[0]);
            Ok(recv.clone())
        } else {
            Ok(RubyValue::Nil)
        }
    }
    // `subtract(enum)` -- removes every element of `enum`, returning self
    // (the in-place counterpart of `-`).
    "subtract"[1] => fn subtract(recv, args, _block) {
        arity!(args, 1);
        check_frozen(recv)?;
        let s = set_of(recv);
        for e in arg_elements(&args[0])? {
            crate::hash_delete(&s.hash, &e);
        }
        Ok(recv.clone())
    }
    // `replace(enum)` -- discards the current members and re-seeds from
    // `enum`, returning self.
    "replace"[1] => fn replace(recv, args, _block) {
        arity!(args, 1);
        check_frozen(recv)?;
        let elements = arg_elements(&args[0])?;
        set_of(recv).replace_contents(elements);
        Ok(recv.clone())
    }
    // `flatten` -- a NEW Set with every nested Set expanded recursively.
    "flatten"[0] => fn flatten(recv, args, _block) {
        arity!(args, 0);
        let mut out = Vec::new();
        flatten_into(set_of(recv), &mut out);
        Ok(set_from(out))
    }
    // `flatten!` -- flattens in place; self if it held any nested Set, else
    // nil (nothing to flatten).
    "flatten!"[0] => fn flatten_bang(recv, args, _block) {
        arity!(args, 0);
        check_frozen(recv)?;
        let s = set_of(recv);
        if !s.elements().iter().any(is_set) {
            return Ok(RubyValue::Nil);
        }
        let mut out = Vec::new();
        flatten_into(s, &mut out);
        s.replace_contents(out);
        Ok(recv.clone())
    }
    // `map!`/`collect!` -- replaces each element with the block's result,
    // in place, returning self (dedup applies to the mapped values).
    "map!"[0] | "collect!"[0] => fn map_bang(recv, args, block) {
        arity!(args, 0);
        check_frozen(recv)?;
        let p = block_or_enum!(recv, "map!", args, block);
        let s = set_of(recv);
        let mut mapped = Vec::new();
        for e in s.elements() {
            mapped.push(p.call(&[e])?);
        }
        s.replace_contents(mapped);
        Ok(recv.clone())
    }
    // `select!`/`filter!` -- keep the elements the block likes; self if any
    // were dropped, else nil.
    "select!"[0] | "filter!"[0] => fn select_bang(recv, args, block) {
        arity!(args, 0);
        check_frozen(recv)?;
        let p = block_or_enum!(recv, "select!", args, block);
        let changed = filter_in_place(recv, &p, true)?;
        Ok(if changed { recv.clone() } else { RubyValue::Nil })
    }
    // `keep_if` -- like `select!` but always returns self.
    "keep_if"[0] => fn keep_if(recv, args, block) {
        arity!(args, 0);
        check_frozen(recv)?;
        let p = block_or_enum!(recv, "keep_if", args, block);
        filter_in_place(recv, &p, true)?;
        Ok(recv.clone())
    }
    // `reject!` -- drop the elements the block likes; self if any were
    // dropped, else nil.
    "reject!"[0] => fn reject_bang(recv, args, block) {
        arity!(args, 0);
        check_frozen(recv)?;
        let p = block_or_enum!(recv, "reject!", args, block);
        let changed = filter_in_place(recv, &p, false)?;
        Ok(if changed { recv.clone() } else { RubyValue::Nil })
    }
    // `delete_if` -- like `reject!` but always returns self.
    "delete_if"[0] => fn delete_if(recv, args, block) {
        arity!(args, 0);
        check_frozen(recv)?;
        let p = block_or_enum!(recv, "delete_if", args, block);
        filter_in_place(recv, &p, false)?;
        Ok(recv.clone())
    }
    // `classify { |o| key }` -- a `Hash{ key => Set }` grouping by block value.
    "classify"[0] => fn classify(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "classify", args, block);
        Ok(RubyValue::Hash(classify_groups(recv, &p)?))
    }
    // `divide` -- partition into a Set of Sets. A one-arg block groups by its
    // value (`classify`'s values); a two-arg block treats `block.call(u,v)` as
    // a directed edge and returns the strongly-connected components.
    "divide"[0] => fn divide(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "divide", args, block);
        if p.arity() == 2 {
            let elements = set_of(recv).elements();
            let n = elements.len();
            let mut adj = vec![Vec::new(); n];
            for (i, u) in elements.iter().enumerate() {
                for (j, v) in elements.iter().enumerate() {
                    if p.call(&[u.clone(), v.clone()])?.truthy() {
                        adj[i].push(j);
                    }
                }
            }
            let components = strongly_connected_components(&adj);
            let sets = components
                .into_iter()
                .map(|component| set_from(component.into_iter().map(|i| elements[i].clone())));
            Ok(set_from(sets))
        } else {
            let groups = classify_groups(recv, &p)?;
            let values: Vec<RubyValue> = groups.lock().values().map(|(_, v)| v.clone()).collect();
            Ok(set_from(values))
        }
    }
    "each"[0] => fn each(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each", args, block);
        for e in set_of(recv).elements() {
            p.call(&[e])?;
        }
        Ok(recv.clone())
    }
    "size"[0] | "length"[0] => fn size(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(set_of(recv).len() as i64))
    }
    // `Set#reset` rebuilds the internal index after elements have been mutated
    // in place. This Set keys elements structurally, so there is nothing to
    // re-index; it answers self, matching CRuby's return.
    "reset"[0] => fn reset(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "empty?"[0] => fn empty_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(set_of(recv).len() == 0))
    }
    "to_a"[0] => fn to_a(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Array(crate::array_new(set_of(recv).elements())))
    }
    // `Set#join(sep = "")` -- delegates to the element array's join.
    "join" => fn join(recv, args, _block) {
        arity!(args, 0..=1);
        let arr = RubyValue::Array(crate::array_new(set_of(recv).elements()));
        crate::dispatch::send_value(&arr, crate::Symbol::intern("join"), args, None)
    }
    "to_set" => fn to_set(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "clear"[0] => fn clear(recv, args, _block) {
        arity!(args, 0);
        check_frozen(recv)?;
        set_of(recv).hash.lock().clear();
        Ok(recv.clone())
    }
    "merge" => fn merge(recv, args, _block) {
        check_frozen(recv)?;
        let s = set_of(recv);
        for a in args {
            for e in arg_elements(a)? {
                s.insert(e);
            }
        }
        Ok(recv.clone())
    }
    "|"[1] | "union"[1] | "+"[1] | "merge_new" => fn union(recv, args, _block) {
        arity!(args, 1);
        let mut out = set_of(recv).elements();
        out.extend(arg_elements(&args[0])?);
        Ok(set_from(out))
    }
    "&"[1] | "intersection"[1] => fn intersection(recv, args, _block) {
        arity!(args, 1);
        let other = set_from(arg_elements(&args[0])?);
        let keep: Vec<RubyValue> = set_of(recv)
            .elements()
            .into_iter()
            .filter(|e| set_of(&other).contains(e))
            .collect();
        Ok(set_from(keep))
    }
    "-"[1] | "difference"[1] => fn difference(recv, args, _block) {
        arity!(args, 1);
        let other = set_from(arg_elements(&args[0])?);
        let keep: Vec<RubyValue> = set_of(recv)
            .elements()
            .into_iter()
            .filter(|e| !set_of(&other).contains(e))
            .collect();
        Ok(set_from(keep))
    }
    "^"[1] => fn symmetric_difference(recv, args, _block) {
        arity!(args, 1);
        let recv_elems = set_of(recv).elements();
        let recv_set = set_from(recv_elems.clone());
        let other_elems = arg_elements(&args[0])?;
        let other_set = set_from(other_elems.clone());
        let mut out: Vec<RubyValue> =
            recv_elems.into_iter().filter(|e| !set_of(&other_set).contains(e)).collect();
        for e in other_elems {
            if !set_of(&recv_set).contains(&e) {
                out.push(e);
            }
        }
        Ok(set_from(out))
    }
    "=="[1] => fn eq(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Object(o) = &args[0] else {
            return Ok(RubyValue::Bool(false));
        };
        if o.class_id() != SET_CLASS {
            return Ok(RubyValue::Bool(false));
        }
        let a = set_of(recv);
        let other = set_of(&args[0]);
        Ok(RubyValue::Bool(a.len() == other.len() && a.elements().iter().all(|e| other.contains(e))))
    }
    "subset?"[1] | "<="[1] => fn subset_p(recv, args, _block) {
        arity!(args, 1);
        let other = coerce_set(&args[0])?;
        let a = set_of(recv);
        Ok(RubyValue::Bool(a.elements().iter().all(|e| set_of(&other).contains(e))))
    }
    "proper_subset?"[1] | "<"[1] => fn proper_subset_p(recv, args, _block) {
        arity!(args, 1);
        let other = coerce_set(&args[0])?;
        let a = set_of(recv);
        let os = set_of(&other);
        Ok(RubyValue::Bool(a.len() < os.len() && a.elements().iter().all(|e| os.contains(e))))
    }
    "superset?"[1] | ">="[1] => fn superset_p(recv, args, _block) {
        arity!(args, 1);
        let other = coerce_set(&args[0])?;
        let os = set_of(&other);
        let a = set_of(recv);
        Ok(RubyValue::Bool(os.elements().iter().all(|e| a.contains(e))))
    }
    "proper_superset?"[1] | ">"[1] => fn proper_superset_p(recv, args, _block) {
        arity!(args, 1);
        let other = coerce_set(&args[0])?;
        let os = set_of(&other);
        let a = set_of(recv);
        Ok(RubyValue::Bool(a.len() > os.len() && os.elements().iter().all(|e| a.contains(e))))
    }
    "disjoint?"[1] => fn disjoint_p(recv, args, _block) {
        arity!(args, 1);
        let a = set_of(recv);
        Ok(RubyValue::Bool(arg_elements(&args[0])?.iter().all(|e| !a.contains(e))))
    }
    "intersect?"[1] => fn intersect_p(recv, args, _block) {
        arity!(args, 1);
        let a = set_of(recv);
        Ok(RubyValue::Bool(arg_elements(&args[0])?.iter().any(|e| a.contains(e))))
    }
    "dup"[0] | "clone" => fn dup(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Object(set_of(recv).dup_object(false)))
    }
    "inspect"[0] | "to_s"[0] => fn inspect(recv, args, _block) {
        arity!(args, 0);
        let parts: Vec<String> =
            set_of(recv).elements().iter().map(|e| e.inspect_string()).collect();
        Ok(RubyValue::Str(crate::string_new(format!("Set[{}]", parts.join(", ")))))
    }
}

/// A subset/superset comparison argument must itself be a Set (CRuby raises
/// ArgumentError for a non-Set here, unlike the algebra operators).
fn coerce_set(v: &RubyValue) -> Result<RubyValue, Signal> {
    match v {
        RubyValue::Object(o) if o.class_id() == SET_CLASS => Ok(v.clone()),
        _ => Err(raise_error("ArgumentError", "value must be a set".to_string())),
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Set.new` / `Set.new(enum)` / `Set.new(enum) { |o| transform(o) }`.
    "new" => fn new_m(_recv, args, block) {
        arity!(args, 0..=1);
        let out = empty_set();
        if let Some(source) = args.first() {
            if !source.is_nil() {
                let s = set_of(&out);
                for e in arg_elements(source)? {
                    let e = match &block {
                        Some(p @ RubyValue::Proc(_)) => {
                            crate::dispatch::send_value(p, crate::Symbol::intern("call"), &[e], None)?
                        }
                        _ => e,
                    };
                    s.insert(e);
                }
            }
        }
        Ok(out)
    }
    // `Set[a, b, c]` -- every argument is a member (deduplicated).
    "[]" => fn brackets(_recv, args, _block) {
        Ok(set_from(args.iter().cloned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(xs: &[i64]) -> RubyValue {
        set_from(xs.iter().map(|&i| RubyValue::Int(i)))
    }

    #[test]
    fn new_deduplicates_and_preserves_insertion_order() {
        let s = new_m(&RubyValue::Nil, &[RubyValue::Array(crate::array_new(
            [3, 1, 3, 2, 1].iter().map(|&i| RubyValue::Int(i)).collect(),
        ))], None).unwrap();
        let RubyValue::Array(a) = to_a(&s, &[], None).unwrap() else { panic!() };
        let got: Vec<i64> = a.lock().iter().map(|v| match v {
            RubyValue::Int(i) => *i,
            _ => panic!(),
        }).collect();
        assert_eq!(got, vec![3, 1, 2]);
    }

    #[test]
    fn membership_and_size() {
        let s = ints(&[1, 2, 3]);
        assert!(matches!(include_p(&s, &[RubyValue::Int(2)], None).unwrap(), RubyValue::Bool(true)));
        assert!(matches!(include_p(&s, &[RubyValue::Int(9)], None).unwrap(), RubyValue::Bool(false)));
        assert!(matches!(size(&s, &[], None).unwrap(), RubyValue::Int(3)));
    }

    #[test]
    fn set_algebra() {
        let a = ints(&[1, 2, 3]);
        let union = union(&a, &[ints(&[3, 4])], None).unwrap();
        assert!(matches!(size(&union, &[], None).unwrap(), RubyValue::Int(4)));
        let inter = intersection(&a, &[ints(&[2, 3, 4])], None).unwrap();
        assert!(matches!(size(&inter, &[], None).unwrap(), RubyValue::Int(2)));
        let diff = difference(&a, &[ints(&[2])], None).unwrap();
        assert!(matches!(size(&diff, &[], None).unwrap(), RubyValue::Int(2)));
        let sym = symmetric_difference(&a, &[ints(&[2, 3, 4])], None).unwrap();
        assert!(matches!(size(&sym, &[], None).unwrap(), RubyValue::Int(2)));
    }

    #[test]
    fn equality_is_structural_and_order_independent() {
        assert!(matches!(eq(&ints(&[1, 2, 3]), &[ints(&[3, 2, 1])], None).unwrap(), RubyValue::Bool(true)));
        assert!(matches!(eq(&ints(&[1, 2]), &[ints(&[1, 2, 3])], None).unwrap(), RubyValue::Bool(false)));
        assert!(matches!(eq(&ints(&[1]), &[RubyValue::Int(1)], None).unwrap(), RubyValue::Bool(false)));
    }

    #[test]
    fn add_p_reports_novelty_and_frozen_is_enforced() {
        let s = ints(&[1, 2]);
        assert!(matches!(add_p(&s, &[RubyValue::Int(2)], None).unwrap(), RubyValue::Nil));
        assert!(matches!(add_p(&s, &[RubyValue::Int(3)], None).unwrap(), RubyValue::Object(_)));
        set_of(&s).set_frozen();
        // A frozen mutation raises; with no registry installed, `raise_error`
        // panics rather than building the exception (the comparable-test
        // convention), so assert the raise via `catch_unwind`.
        let frozen = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            add(&s, &[RubyValue::Int(4)], None)
        }));
        assert!(frozen.is_err());
    }

    #[test]
    fn bang_filters_report_change_and_flatten_reports_nesting() {
        // `select!` returns self when it removed something, nil when it did not.
        let s = ints(&[1, 2, 3, 4]);
        assert!(matches!(select_bang(&s, &[], pred(|x| x % 2 == 0)).unwrap(), RubyValue::Object(_)));
        let done = ints(&[2, 4]);
        assert!(matches!(select_bang(&done, &[], pred(|x| x % 2 == 0)).unwrap(), RubyValue::Nil));
        // `flatten!` returns nil on a flat Set.
        assert!(matches!(flatten_bang(&ints(&[1, 2]), &[], None).unwrap(), RubyValue::Nil));
        let nested = set_from([ints(&[1, 2]), ints(&[3])]);
        assert!(matches!(flatten_bang(&nested, &[], None).unwrap(), RubyValue::Object(_)));
        assert_eq!(elems_sorted(&nested), vec![1, 2, 3]);
    }

    #[test]
    fn divide_by_value_and_by_connected_components() {
        // One-arg block groups by return value.
        let by_value = divide(&ints(&[1, 2, 3, 4]), &[], block(|x| x % 3)).unwrap();
        assert!(matches!(size(&by_value, &[], None).unwrap(), RubyValue::Int(3)));
        // Two-arg block: 1-2-3-4 chain is one strongly-connected component.
        let chain = divide(&ints(&[1, 2, 3, 4]), &[], block2(|x, y| (x - y).abs() == 1)).unwrap();
        assert!(matches!(size(&chain, &[], None).unwrap(), RubyValue::Int(1)));
    }

    fn elems_sorted(s: &RubyValue) -> Vec<i64> {
        let mut got: Vec<i64> = set_of(s).elements().iter().map(|v| match v {
            RubyValue::Int(i) => *i,
            _ => panic!(),
        }).collect();
        got.sort();
        got
    }

    /// A one-arg Int->Int test block wrapped as a `Proc` value.
    fn block(f: impl Fn(i64) -> i64 + Send + Sync + 'static) -> Option<RubyValue> {
        Some(RubyValue::Proc(crate::RProc::with_meta(move |args| {
            let RubyValue::Int(x) = args[0] else { panic!() };
            Ok(RubyValue::Int(f(x)))
        }, 1, false)))
    }

    /// A one-arg Int predicate test block (returns a Bool, so truthiness is
    /// meaningful) wrapped as a `Proc` value.
    fn pred(f: impl Fn(i64) -> bool + Send + Sync + 'static) -> Option<RubyValue> {
        Some(RubyValue::Proc(crate::RProc::with_meta(move |args| {
            let RubyValue::Int(x) = args[0] else { panic!() };
            Ok(RubyValue::Bool(f(x)))
        }, 1, false)))
    }

    /// A two-arg Int,Int->bool test block wrapped as a `Proc` value.
    fn block2(f: impl Fn(i64, i64) -> bool + Send + Sync + 'static) -> Option<RubyValue> {
        Some(RubyValue::Proc(crate::RProc::with_meta(move |args| {
            let (RubyValue::Int(x), RubyValue::Int(y)) = (&args[0], &args[1]) else { panic!() };
            Ok(RubyValue::Bool(f(*x, *y)))
        }, 2, false)))
    }

    #[test]
    fn inspect_renders_bracket_form() {
        let RubyValue::Str(s) = inspect(&ints(&[1, 2, 3]), &[], None).unwrap() else { panic!() };
        assert_eq!(s.lock().to_utf8_lossy(), "Set[1, 2, 3]");
        let RubyValue::Str(e) = inspect(&ints(&[]), &[], None).unwrap() else { panic!() };
        assert_eq!(e.lock().to_utf8_lossy(), "Set[]");
    }
}
