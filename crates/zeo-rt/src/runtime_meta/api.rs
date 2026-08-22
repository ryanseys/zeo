//! The public runtime API: what the builtins and codegen call to define,
//! alias, undef, and mint classes/methods at run time.

use super::*;

/// `some_class.define_method(name) { body }` -- install/override an instance
/// method on the class with id `id` (frozen or runtime). Returns the name.
/// A `Foo.freeze`d class refuses (`can't modify frozen Class: Foo`,
/// CRuby's guard on every method-table mutation).
pub fn runtime_define_method(id: ClassId, name: Symbol, body: RProc) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // A method defined on an object's singleton class (`obj.singleton_class`)
    // is a per-object singleton, not an instance method of a shared class --
    // redirect to the owner. For a class owner it becomes a class method,
    // carrying the body's visibility cursor: a live `class_eval` frame, or
    // the persistent default a bare `private` in a compiled singleton body
    // stored (see `runtime_set_visibility`).
    let owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = owner {
        let vis = current_frame_for(id).map(|f| f.vis).or_else(|| {
            maps()
                .classes
                .read()
                .unwrap()
                .get(&id.0)
                .and_then(|e| e.singleton_default_vis)
        });
        let out = runtime_define_singleton_method(&owner, name, body)?;
        if let (Some(v), RubyValue::Class(cid)) = (vis, &owner) {
            runtime_class_method_visibility(
                *cid,
                &[RubyValue::Symbol(name)],
                v != crate::dispatch::MethodVisibility::Public,
            )?;
        }
        return Ok(out);
    }
    crate::method_meta::record_runtime_params(id, crate::MethodKind::Instance, name, &body);
    let m = dynamic_from_proc(id, name, body.clone());
    let frame = current_frame_for(id);
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods.insert(name, m);
        e.value_bodies.insert(name, body);
        e.undefs.remove(&name);
        e.removed.remove(&name);
        // `initialize` and its copy/clone/dup family are private wherever
        // they are defined -- CRuby stamps them so in `rb_method_entry_make`
        // regardless of the visibility cursor.
        let always_private = matches!(
            name.name_str(),
            "initialize" | "initialize_copy" | "initialize_clone" | "initialize_dup"
        );
        match frame {
            _ if always_private => {
                e.methods_vis
                    .insert(name, crate::dispatch::MethodVisibility::Private);
            }
            // Outside a class body a runtime definition is public -- drop any
            // earlier `private :name` mark.
            None => {
                e.methods_vis.remove(&name);
            }
            // `module_function` makes the instance copy private and adds a
            // public module method; a bare `private`/`protected` just marks.
            Some(f) if f.module_function => {
                e.methods_vis
                    .insert(name, crate::dispatch::MethodVisibility::Private);
            }
            Some(f) if f.vis != crate::dispatch::MethodVisibility::Public => {
                e.methods_vis.insert(name, f.vis);
            }
            Some(_) => {
                e.methods_vis.remove(&name);
            }
        }
    }
    // The module-method half is built with the overlay lock DROPPED:
    // `extended_class_method` reads the overlay itself.
    if frame.is_some_and(|f| f.module_function)
        && let Some(wrapper) = extended_class_method(id, name)
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.class_methods.insert(name, wrapper);
        e.extended_class_methods.remove(&name);
    }
    patch_class(id);
    mark_live();
    // A hook name defined on Module/Class/BasicObject itself applies to every
    // class; nothing downstream could infer that from the owner id.
    if global_def_hook_owner(id, name) {
        mark_global_def_hook(name.name_str());
    }
    fire_def_hook(DefTarget::Class(id), DefEvent::Added, name)?;
    // `module_function` adds a module method too, and ruby reports BOTH: the
    // instance copy through `method_added`, then the module copy through
    // `singleton_method_added`.
    if frame.is_some_and(|f| f.module_function) {
        fire_def_hook(
            DefTarget::Singleton(&RubyValue::Class(id)),
            DefEvent::Added,
            name,
        )?;
    }
    Ok(RubyValue::Symbol(name))
}

/// Seeds the singleton mint with a COMPILE-registered singleton class: the
/// surrogate a constant-bearing `class << self` body registered for `owner`
/// (its constants live on the surrogate's id in the frozen tables). After
/// this, `owner.singleton_class` answers the surrogate, and a runtime
/// `def owner.x` through it redirects to `owner` exactly as a minted
/// singleton would (`singleton_owner`). Called from generated `main()`
/// before the first statement runs -- no gates need flipping, because
/// nothing here adds an overlay method table.
pub fn register_singleton_surrogate(owner: ClassId, surrogate: ClassId) {
    let owner_val = RubyValue::Class(owner);
    let Some(key) = singleton_class_key(&owner_val) else {
        return;
    };
    maps()
        .singleton_classes
        .write()
        .unwrap()
        .insert(key, surrogate);
    maps()
        .singleton_owner
        .write()
        .unwrap()
        .insert(surrogate.0, owner_val);
}

/// Installs a COMPILED trampoline as the current body of `id`'s `name`.
///
/// This is the runtime half of a positional method redefinition: a reopen's
/// `def` over an existing method, or a `def x; def x` pair that a
/// `method_added` hook observes. The static tables keep the final body.
/// Codegen calls this at boot with the FIRST body, and again at each
/// redefinition's document position, so dynamic dispatch tracks Ruby's
/// install-where-it-stands timeline.
///
/// No hook fires here. The spliced `DefHook` at the same position is the
/// report, exactly as for a statically-registered `def`.
pub fn runtime_replace_method(id: ClassId, name: Symbol, f: crate::dispatch::MethodFn) {
    replace_method_impl(id, name, crate::dispatch::MethodImpl::Static(f));
}

/// [`runtime_replace_method`]'s Cranelift twin: the body is a compiled
/// `ValueFn`, so the overlay entry is a `CValue`.
pub fn runtime_replace_method_c(id: ClassId, name: Symbol, f: crate::capi::ValueFn) {
    replace_method_impl(id, name, crate::dispatch::MethodImpl::CValue(f));
}

fn replace_method_impl(id: ClassId, name: Symbol, imp: crate::dispatch::MethodImpl) {
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods.insert(name, imp.clone());
        e.undefs.remove(&name);
        e.removed.remove(&name);
    }
    // A per-object `extend` COPIES the module's rows into that object's
    // singleton table, so nothing above reaches them. `extended_names` records
    // which module supplied each copied name, which is exactly what a
    // refresh needs -- and an object whose OWN `def` later claimed the name
    // has already been cleared from that table, so it keeps its own body.
    refresh_extended_copies(id, name, &imp);
    patch_class(id);
    mark_live();
}

/// Re-copy `name`'s new body into every per-object singleton table that took
/// it from module `id`. A no-op for a class (nothing extends one per-object)
/// and for a module nothing has extended.
fn refresh_extended_copies(id: ClassId, name: Symbol, imp: &crate::dispatch::MethodImpl) {
    let keys: Vec<usize> = {
        let r = maps().extended_names.read().unwrap();
        r.iter()
            .filter(|(_, t)| t.get(&name) == Some(&id))
            .map(|(&k, _)| k)
            .collect()
    };
    if keys.is_empty() {
        return;
    }
    let mut w = maps().singletons.write().unwrap();
    for key in keys {
        if let Some(table) = w.get_mut(&key) {
            table.insert(name, imp.clone());
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum AttrKind {
    Reader,
    Writer,
    Accessor,
}

/// `attr_reader`/`attr_writer`/`attr_accessor` reached AT RUNTIME
/// (`Class.new { attr_reader :v }`, `Foo.class_eval { attr_accessor :y }`).
/// The literal class-body form expands to real `def`s at compile time.
///
/// Each accessor closes over the RECEIVER's name-keyed ivar storage, which is
/// total across every receiver kind -- a `DynObject`, a generated struct's
/// typed field or `__overflow` map, a `ValueSubclass` -- so one implementation
/// serves a runtime class and a `class_eval` over a compiled one alike.
pub fn runtime_attr(id: ClassId, args: &[RubyValue], kind: AttrKind) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // An attr defined on a SINGLETON class is a singleton attr on its owner,
    // reading that owner's own ivars -- not an instance method of a shared
    // class. `runtime_define_method` redirects the same way, and for the same
    // reason. minitest's `cattr_accessor` is written exactly this way:
    // `(class << self; self; end).attr_accessor name`.
    let owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = owner {
        return singleton_attr(&owner, args, kind);
    }
    let mut defined = Vec::new();
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if kind != AttrKind::Writer {
            let key: Arc<str> = Arc::from(name.name().as_str());
            let getter =
                MethodImpl::Dynamic(Arc::new(move |recv: &RObj, args: &[RubyValue], _| {
                    if !args.is_empty() {
                        return Err(arg_error!(
                            "wrong number of arguments (given {}, expected 0)",
                            args.len()
                        ));
                    }
                    Ok(recv.ivar_get_named(&key).unwrap_or(RubyValue::Nil))
                }));
            install_attr(id, name, getter);
            defined.push(RubyValue::Symbol(name));
        }
        if kind != AttrKind::Reader {
            let key: Arc<str> = Arc::from(name.name().as_str());
            let setter_name = Symbol::intern(&format!("{}=", name.name()));
            let setter =
                MethodImpl::Dynamic(Arc::new(move |recv: &RObj, args: &[RubyValue], _| {
                    let [v] = args else {
                        return Err(arg_error!(
                            "wrong number of arguments (given {}, expected 1)",
                            args.len()
                        ));
                    };
                    recv.ivar_set_named(&key, v.clone());
                    Ok(v.clone())
                }));
            install_attr(id, setter_name, setter);
            defined.push(RubyValue::Symbol(setter_name));
        }
    }
    patch_class(id);
    mark_live();
    // One hook per generated name, reader before writer -- ruby reports
    // `attr_accessor :c` as `method_added(:c)` then `method_added(:c=)`.
    for sym in &defined {
        let RubyValue::Symbol(sym) = sym else {
            continue;
        };
        fire_def_hook(DefTarget::Class(id), DefEvent::Added, *sym)?;
    }
    Ok(RubyValue::Array(crate::array_new(defined)))
}

/// [`runtime_attr`] for a singleton class: each accessor becomes a SINGLETON
/// method on the owner, over the owner's own ivars (`ivar_get_dyn` reaches a
/// class's ivar table and an object's alike). The bodies are `RProc`s rather
/// than the `MethodImpl::Dynamic` the instance-method path builds, because a
/// class-method receiver is a `RubyValue::Class` and has no `RObj` to bind.
fn singleton_attr(
    owner: &RubyValue,
    args: &[RubyValue],
    kind: AttrKind,
) -> Result<RubyValue, Signal> {
    let mut defined = Vec::new();
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if kind != AttrKind::Writer {
            let key: Arc<str> = Arc::from(name.name().as_str());
            let getter = RProc::with_self(
                move |slf: &RubyValue, args: &[RubyValue]| {
                    if !args.is_empty() {
                        return Err(arg_error!(
                            "wrong number of arguments (given {}, expected 0)",
                            args.len()
                        ));
                    }
                    Ok(crate::dispatch::ivar_get_dyn(slf, &key))
                },
                owner.clone(),
                0,
                true,
            );
            runtime_define_singleton_method(owner, name, getter)?;
            defined.push(RubyValue::Symbol(name));
        }
        if kind != AttrKind::Reader {
            let key: Arc<str> = Arc::from(name.name().as_str());
            let setter_name = Symbol::intern(&format!("{}=", name.name()));
            let setter = RProc::with_self(
                move |slf: &RubyValue, args: &[RubyValue]| {
                    let [v] = args else {
                        return Err(arg_error!(
                            "wrong number of arguments (given {}, expected 1)",
                            args.len()
                        ));
                    };
                    crate::dispatch::ivar_set_dyn(slf, &key, v.clone())
                },
                owner.clone(),
                1,
                true,
            );
            runtime_define_singleton_method(owner, setter_name, setter)?;
            defined.push(RubyValue::Symbol(setter_name));
        }
    }
    mark_live();
    Ok(RubyValue::Array(crate::array_new(defined)))
}

fn install_attr(id: ClassId, name: Symbol, m: MethodImpl) {
    let mut w = maps().classes.write().unwrap();
    let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
    e.methods.insert(name, m);
    e.methods_vis.remove(&name);
    e.undefs.remove(&name);
    e.removed.remove(&name);
}

/// `Module#undef_method` -- CRuby's `rb_undef`. The name must currently
/// RESOLVE for instances of `id`; the entry then terminates the MRO walk here,
/// so an ancestor's definition can no longer answer for `id`.
pub fn runtime_undef_method(id: ClassId, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // `class << self; undef x; end` reaches here with the SINGLETON's id. Its
    // instance methods are the owner's class methods, so the retirement belongs
    // in the owner's class-method space -- see `OverlayEntry::class_undefs`.
    if let Some(owner) = singleton_class_owner(id) {
        return runtime_undef_class_method(owner, id, args);
    }
    // The same redirect for an ORDINARY object's singleton class, which
    // `singleton_class_owner` cannot answer for (it names a class). Its
    // instance methods are that one object's singleton methods, so the
    // retirement is that one object's too -- and it has to be recorded, not
    // just applied: the class still defines the name, and the tombstone is the
    // only thing that says this object no longer answers it.
    let owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = owner
        && !matches!(owner, RubyValue::Class(_))
    {
        return runtime_undef_singleton_method(&owner, id, args);
    }
    let mut undefined = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if !crate::dispatch::responds_to(id, name, true) {
            return Err(name_error!(
                "undefined method '{}' for class '{}'",
                name.name(),
                crate::dispatch::class_name(id).unwrap_or_else(|| "?".to_string())
            ));
        }
        {
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
            e.undefs.insert(name);
            e.methods.remove(&name);
        }
        undefined.push(name);
    }
    patch_class(id);
    mark_live();
    for name in undefined {
        fire_def_hook(DefTarget::Class(id), DefEvent::Undefined, name)?;
    }
    Ok(RubyValue::Class(id))
}

/// [`runtime_undef_method`] for ONE OBJECT, reached through that object's
/// singleton class (`g.singleton_class.undef_method(:close)`, and the
/// `class << g; undef :close; end` that spells the same thing).
///
/// Writes the tombstone `singleton_undefs` holds and takes any singleton method
/// of that name with it. `singleton` is the id the call came in on, and it
/// decides what EXISTS: undefining a name the object cannot answer at all is
/// ruby's NameError, and it names the singleton class the caller reached
/// through.
fn runtime_undef_singleton_method(
    owner: &RubyValue,
    singleton: ClassId,
    args: &[RubyValue],
) -> Result<RubyValue, Signal> {
    let Some(key) = pin_identity(owner) else {
        return Err(type_error!("can't define singleton"));
    };
    let mut undefined = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if !crate::dispatch::responds_to_value(owner, name, true) {
            return Err(name_error!(
                "undefined method '{}' for class '{}'",
                name.name(),
                crate::dispatch::class_name(singleton).unwrap_or_else(|| "?".to_string())
            ));
        }
        {
            let mut w = maps().singletons.write().unwrap();
            if let Some(t) = w.get_mut(&key) {
                t.remove(&name);
            }
        }
        {
            let mut w = maps().value_singletons.write().unwrap();
            if let Some(t) = w.get_mut(&key) {
                t.remove(&name);
            }
        }
        clear_extended_name(key, name);
        maps()
            .singleton_undefs
            .write()
            .unwrap()
            .entry(key)
            .or_default()
            .insert(name);
        undefined.push(name);
    }
    mark_singletons();
    mark_live();
    for name in undefined {
        fire_def_hook(DefTarget::Singleton(owner), DefEvent::Undefined, name)?;
    }
    Ok(RubyValue::Class(singleton))
}

/// [`runtime_alias_method`] for ONE OBJECT, reached through that object's
/// singleton class (`class << obj; alias shut close; end`).
///
/// `old` is resolved the way the OBJECT answers it -- its own singleton table
/// first, then its class's chain -- because that is what ruby copies: an alias
/// takes the definition the receiver would have run.
fn runtime_alias_singleton_method(
    owner: &RubyValue,
    singleton: ClassId,
    new: Symbol,
    old: Symbol,
) -> Result<RubyValue, Signal> {
    let Some(key) = pin_identity(owner) else {
        return Err(type_error!("can't define singleton"));
    };
    let own = maps()
        .singletons
        .read()
        .unwrap()
        .get(&key)
        .and_then(|t| t.get(&old).cloned());
    let Some(m) = own.or_else(|| snapshot_instance_method(owner.class_id(), old)) else {
        return Err(name_error!(
            "undefined method '{}' for class '{}'",
            old.name(),
            crate::dispatch::class_name(singleton).unwrap_or_default()
        ));
    };
    maps()
        .singletons
        .write()
        .unwrap()
        .entry(key)
        .or_default()
        .insert(new, m);
    // An alias DEFINES the new name, so it lifts any tombstone standing over it.
    if let Some(t) = maps().singleton_undefs.write().unwrap().get_mut(&key) {
        t.remove(&new);
    }
    mark_singletons();
    mark_live();
    Ok(RubyValue::Symbol(new))
}

/// Whether `recv` retired `name` for itself -- see `singleton_undefs`. Always
/// behind `is_live()`, like every other identity-keyed probe: an ordinary
/// program never takes the hash lookup.
pub fn object_method_undefined(recv: &RubyValue, name: Symbol) -> bool {
    let u = maps().singleton_undefs.read().unwrap();
    if u.is_empty() {
        return false;
    }
    value_identity(recv).is_some_and(|k| u.get(&k).is_some_and(|t| t.contains(&name)))
}

/// [`runtime_undef_method`] for a CLASS method, reached through the owner's
/// singleton class. Retires the name for `owner` and every subclass, exactly as
/// the instance-method form does.
///
/// `singleton` is the id the call came in on, and it decides what EXISTS here.
/// A singleton class inherits `Module`'s instance methods -- `#<Class:M>`'s
/// ancestors are `[#<Class:M>, Module, Object, Kernel, BasicObject]` -- and
/// ruby's `undef` retires an inherited method as readily as an own one
/// (`rb_undef` resolves the name with `rb_method_entry`, which walks the whole
/// chain). Checking only the owner's CLASS-method space missed that half: the
/// singleton gem's `undef_method :extend_object`, written inside an `extended`
/// hook, raised NameError for a method `private_method_defined?` reported on
/// the very same receiver.
/// An `undef` written inside a REOPENED `class << self`, applied where it
/// stands. The compile-time form (`ClassInfo::class_undefined`) applies from
/// program start, which is wrong the moment a call sits between the two
/// bodies: `def self.away` ... `p Gone.away` ... `class << self; undef away`.
///
/// Goes through the same singleton tombstone the runtime spelling writes, so
/// dispatch and every ancestor-walking reader agree.
pub fn runtime_undef_class_method_names(id: ClassId, names: &[&str]) -> Result<(), Signal> {
    let singleton = singleton_class_id_of(id);
    let args: Vec<RubyValue> = names
        .iter()
        .map(|n| RubyValue::Symbol(Symbol::intern(n)))
        .collect();
    runtime_undef_class_method(id, singleton, &args).map(|_| ())
}

fn runtime_undef_class_method(
    owner: ClassId,
    singleton: ClassId,
    args: &[RubyValue],
) -> Result<RubyValue, Signal> {
    let mut undefined = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        if crate::dispatch::class_method_owner(owner, name).is_none()
            && overlay_class_method(owner, name).is_none()
            && crate::dispatch::instance_method_visibility(singleton, name).is_none()
        {
            return Err(name_error!(
                "undefined method '{}' for class '{}'",
                name.name(),
                crate::dispatch::class_name(owner).unwrap_or_else(|| "?".to_string())
            ));
        }
        {
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(owner.0).or_insert_with(OverlayEntry::delta);
            e.class_undefs.insert(name);
            e.class_methods.remove(&name);
            // And a tombstone on the SINGLETON class itself, which is where
            // ruby puts it -- `rb_undef` writes the undef entry into the
            // singleton's own method table, so it shadows the rest of that
            // chain (`Module`, `Class`, `Object`, `Kernel`). `class_undefs`
            // above gates class-method DISPATCH, which is the owner's space;
            // this one is what every ancestor-walking reader already
            // consults, so `method_defined?`, `private_method_defined?`,
            // `instance_method` and `respond_to?` all agree with dispatch
            // instead of finding `Module`'s definition again behind it.
            let s = w.entry(singleton.0).or_insert_with(OverlayEntry::delta);
            s.undefs.insert(name);
            s.methods.remove(&name);
        }
        undefined.push(name);
    }
    patch_class(owner);
    patch_class(singleton);
    mark_live();
    for name in undefined {
        fire_def_hook(DefTarget::Class(owner), DefEvent::Undefined, name)?;
    }
    Ok(RubyValue::Class(owner))
}

/// Whether an `undef` inside `class << self` retired `name` as a CLASS method
/// of `id` -- the gate every class-method resolution owes
/// [`OverlayEntry::class_undefs`].
/// The class-method names `id`'s own entry retired -- see
/// [`OverlayEntry::class_undefs`].
pub(crate) fn overlay_class_undefs(id: ClassId) -> Vec<Symbol> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .map(|e| e.class_undefs.iter().copied().collect())
        .unwrap_or_default()
}

pub(crate) fn class_method_undefined(id: ClassId, name: Symbol) -> bool {
    // Every ancestor, not just `id`: a class method is inherited through the
    // parallel singleton chain, so `class Multi; class << self; undef x; end;
    // end` retires it for `Sub < Multi` too. A subclass that DEFINES the name
    // again ends the walk -- its own `def self.x` sits nearer than the
    // tombstone, exactly as it would in ruby.
    let c = maps().classes.read().unwrap();
    for &anc in crate::dispatch::ancestors_of_value(id) {
        if c.get(&anc.0)
            .is_some_and(|e| e.class_undefs.contains(&name))
        {
            return true;
        }
        if anc != id && crate::dispatch::class_defines_own_class_method(anc, name) {
            return false;
        }
    }
    false
}

/// `Module#remove_method` -- drops this class's OWN definition, leaving an
/// inherited one reachable. Unlike `undef_method` it plants no terminator:
/// the walk skips this class's tables for the name and carries on.
///
/// It has to plant SOMETHING, though, and that is [`OverlayEntry::removed`].
/// A class's own definition can live in two layers -- the runtime overlay and
/// the COMPILED registry row -- so deleting the overlay entry alone merely
/// uncovered the compiled row underneath, and `remove_method` looked like it
/// had done nothing.
pub fn runtime_remove_method(id: ClassId, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // A SINGLETON class's instance methods are its owner's CLASS methods, so
    // the removal belongs in the owner's class-method space -- the same two
    // redirects `runtime_undef_method` makes, for the same reason.
    if let Some(owner) = singleton_class_owner(id) {
        return runtime_remove_class_method(owner, id, args);
    }
    let value_owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = value_owner
        && !matches!(owner, RubyValue::Class(_))
    {
        return runtime_remove_singleton_method(&owner, id, args);
    }
    let mut removed_names = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        let in_overlay = maps()
            .classes
            .read()
            .unwrap()
            .get(&id.0)
            .is_some_and(|e| e.methods.contains_key(&name));
        if !in_overlay && !crate::dispatch::class_defines_own_instance_method(id, name) {
            return Err(name_error!(
                "method '{}' not defined in {}",
                name.name(),
                crate::dispatch::class_name(id).unwrap_or_else(|| "?".to_string())
            ));
        }
        removed_names.push(name);
    }
    // A removal on a MODULE reaches the classes that mixed it in. Analyze
    // materializes a module's rows onto each host at compile time, so
    // `Host.new.m` resolves through the HOST's own row and never asks the
    // module -- and a tombstone keyed on the module sits at a position the
    // lookup does not visit.
    //
    // The hosts are named BEFORE the module's own tombstone lands: the
    // question "whose copy is this" is `method_owner`, and once the overlay
    // is live that walk SKIPS a removed position -- so writing first made the
    // module stop being the owner and the sweep find nobody.
    let hosts = mixin_hosts(id, &removed_names);
    {
        let mut w = maps().classes.write().unwrap();
        for &name in &removed_names {
            let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
            e.methods.remove(&name);
            e.value_bodies.remove(&name);
            e.methods_vis.remove(&name);
            e.removed.insert(name);
        }
        for host in &hosts {
            let e = w.entry(host.0).or_insert_with(OverlayEntry::delta);
            for &name in &removed_names {
                e.removed.insert(name);
            }
        }
    }
    for host in hosts {
        // The host's own caches answered before the removal and would keep
        // answering: a reflection read taken ahead of it fills them.
        patch_class(host);
    }
    patch_class(id);
    mark_live();
    for name in removed_names {
        fire_def_hook(DefTarget::Class(id), DefEvent::Removed, name)?;
    }
    Ok(RubyValue::Class(id))
}

/// The classes that carry `mid`'s copy of one of `names` as their own row --
/// every class whose chain holds `mid` and whose winner for the name IS
/// `mid`. A host with a definition of its own is not one: ruby leaves that
/// standing when the module's copy goes.
///
/// `mid` must be a module; a class is nobody's mixin, and the sweep is
/// skipped for one.
fn mixin_hosts(mid: ClassId, names: &[Symbol]) -> Vec<ClassId> {
    if names.is_empty() || crate::dispatch::class_is_module(mid) != Some(true) {
        return Vec::new();
    }
    let mut hosts: Vec<ClassId> = crate::dispatch::classes_with_ancestor(mid)
        .into_iter()
        .map(ClassId)
        .collect();
    {
        let r = maps().classes.read().unwrap();
        for (&id, e) in r.iter() {
            if e.ancestors.contains(&mid) && !hosts.contains(&ClassId(id)) {
                hosts.push(ClassId(id));
            }
        }
    }
    hosts.retain(|&h| {
        h != mid
            && names
                .iter()
                .any(|&n| crate::dispatch::method_owner(h, n) == Some(mid))
    });
    hosts
}

/// [`runtime_remove_method`] reached through a class's SINGLETON class, where
/// the names are `owner`'s class methods -- `runtime_undef_class_method`'s
/// twin, minus the tombstones that stop the walk.
fn runtime_remove_class_method(
    owner: ClassId,
    singleton: ClassId,
    args: &[RubyValue],
) -> Result<RubyValue, Signal> {
    let mut removed_names = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        let in_overlay = overlay_class_method(owner, name).is_some();
        if !in_overlay && !crate::dispatch::class_defines_own_class_method(owner, name) {
            return Err(name_error!(
                "method '{}' not defined in {}",
                name.name(),
                crate::dispatch::class_name(singleton).unwrap_or_else(|| "?".to_string())
            ));
        }
        {
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(owner.0).or_insert_with(OverlayEntry::delta);
            e.class_methods.remove(&name);
            e.extended_class_methods.remove(&name);
            e.class_methods_vis.remove(&name);
            e.class_removed.insert(name);
            // And on the SINGLETON's own id, which is what every
            // ancestor-walking READER consults -- so `instance_methods(false)`
            // and `method_defined?` agree with dispatch instead of finding the
            // row again behind it.
            let sg = w.entry(singleton.0).or_insert_with(OverlayEntry::delta);
            sg.methods.remove(&name);
            sg.value_bodies.remove(&name);
            sg.removed.insert(name);
        }
        removed_names.push(name);
    }
    patch_class(owner);
    patch_class(singleton);
    mark_live();
    for name in removed_names {
        fire_def_hook(DefTarget::Class(owner), DefEvent::Removed, name)?;
    }
    Ok(RubyValue::Class(owner))
}

/// [`runtime_remove_method`] for ONE OBJECT, reached through that object's
/// singleton class. Its methods live identity-keyed with no compiled layer
/// under them, so this is a plain deletion -- the object's CLASS answers
/// afterwards, which is exactly what `remove_method` means.
fn runtime_remove_singleton_method(
    owner: &RubyValue,
    singleton: ClassId,
    args: &[RubyValue],
) -> Result<RubyValue, Signal> {
    let Some(key) = pin_identity(owner) else {
        return Err(type_error!("can't define singleton"));
    };
    let mut removed_names = Vec::with_capacity(args.len());
    for arg in args {
        let name = coerce_method_name(Some(arg))?;
        let gone = {
            let mut w = maps().singletons.write().unwrap();
            w.get_mut(&key).is_some_and(|t| t.remove(&name).is_some())
        } | {
            let mut w = maps().value_singletons.write().unwrap();
            w.get_mut(&key).is_some_and(|t| t.remove(&name).is_some())
        };
        if !gone {
            return Err(name_error!(
                "method '{}' not defined in {}",
                name.name(),
                crate::dispatch::class_name(singleton).unwrap_or_else(|| "?".to_string())
            ));
        }
        clear_extended_name(key, name);
        removed_names.push(name);
    }
    mark_singletons();
    for name in removed_names {
        fire_def_hook(DefTarget::Singleton(owner), DefEvent::Removed, name)?;
    }
    Ok(RubyValue::Class(singleton))
}

/// `Module#alias_method(new, old)` reached AT RUNTIME (computed names --
/// e.g. ostruct's `instance_methods.each { |m| alias_method "#{m}!", m }`
/// bulk loop; the literal-symbol class-body form resolves at compile time).
/// The method `old` resolves to for instances of `id` is SNAPSHOTTED and
/// installed under `new` in the overlay -- CRuby's copy-the-method-entry
/// semantics (`rb_alias`), so a later runtime redefinition of `old` does
/// not change the alias. Returns the new name's Symbol. The alias inherits
/// `old`'s visibility (CRuby: the copied method entry keeps its flags).
pub fn runtime_alias_method(id: ClassId, new: Symbol, old: Symbol) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // A SINGLETON class's instance methods are its owner's CLASS methods, so an
    // alias written there copies one of those -- securerandom's `class << self;
    // begin; Random.urandom(1); alias gen_random gen_random_urandom; rescue`.
    // Resolved and installed on the owner's class-method side, since the
    // singleton id has no instance table of its own.
    if let Some(owner) = singleton_class_owner(id) {
        let existing = maps()
            .classes
            .read()
            .unwrap()
            .get(&owner.0)
            .and_then(|e| e.class_methods.get(&old).cloned());
        let source = existing
            .or_else(|| {
                crate::dispatch::class_method_fn(owner, old)
                    .map(|f| RProc::with_self_and_block(f.into_fn(), RubyValue::Nil, -1, true))
            })
            .or_else(|| extended_class_method(owner, old));
        let Some(source) = source else {
            return Err(name_error!(
                "undefined method '{}' for class '{}'",
                old.name(),
                crate::dispatch::class_name(id).unwrap_or_default()
            ));
        };
        {
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(owner.0).or_insert_with(OverlayEntry::delta);
            e.class_methods.insert(new, source);
            e.extended_class_methods.remove(&new);
        }
        patch_class(owner);
        mark_live();
        return Ok(RubyValue::Symbol(new));
    }
    // The same redirect for an ORDINARY object's singleton class, which
    // `singleton_class_owner` cannot answer for (it names a class). `class <<
    // obj; alias shut close; end` aliases a method for THAT ONE OBJECT, so the
    // copy belongs in its singleton table -- the walk below would instead write
    // it into an instance table the singleton id never had, where no send for
    // that object ever looks, and `obj.shut` raised NoMethodError.
    let value_owner = maps().singleton_owner.read().unwrap().get(&id.0).cloned();
    if let Some(owner) = value_owner
        && !matches!(owner, RubyValue::Class(_))
    {
        return runtime_alias_singleton_method(&owner, id, new, old);
    }
    let snapshot = snapshot_instance_method(id, old).or_else(|| {
        // A parse-special Kernel source (`alias_method :block_given!,
        // :block_given?` reached at runtime): statically-resolved call sites
        // compile these directly, so there is no dispatch row to snapshot --
        // but the alias itself is valid, exactly as `validate_aliases`
        // accepts the static form. Accept it with a stub that only raises if
        // a call actually arrives dynamically (which zeo cannot serve: the
        // answer lives in the CALLER's compiled frame).
        crate::dispatch::PARSE_SPECIAL_KERNEL
            .contains(&old.name().as_str())
            .then(|| {
                MethodImpl::Dynamic(Arc::new(
                    move |_recv: &RObj, _args: &[RubyValue], _block: Option<RubyValue>| {
                        Err(crate::builtins::not_impl_error!(
                            "'{}' cannot be called through a runtime alias (zeo limitation: \
                             it resolves in the caller's compiled frame)",
                            old.name()
                        ))
                    },
                ))
            })
    });
    let Some(m) = snapshot else {
        let kind = if crate::dispatch::class_is_module(id).unwrap_or(false) {
            "module"
        } else {
            "class"
        };
        let cls = crate::dispatch::class_name(id).unwrap_or_default();
        return Err(name_error!(
            "undefined method '{}' for {kind} '{cls}'",
            old.name()
        ));
    };
    // The alias inherits its source's CURRENT visibility (CRuby: the copied
    // method entry keeps its flags) -- resolved before install so a stale
    // mark under `new` can't shadow it.
    let vis = crate::dispatch::instance_method_visibility(id, old);
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods.insert(new, m);
        match vis {
            Some(v) => e.methods_vis.insert(new, v),
            None => e.methods_vis.remove(&new),
        };
    }
    // Reflection follows the copy: the alias reports the source's birth name
    // (so a chain -- `alias b a; alias c b` -- still answers `:a`) and the
    // source's own signature and definition site.
    let origin = crate::method_meta::original_name(id, crate::MethodKind::Instance, old);
    let mut meta = crate::MethodMeta::instance(id.0, &new.name()).aliased_from(&origin.name());
    if let Some(source) = crate::method_meta::lookup(id, crate::MethodKind::Instance, old) {
        meta = meta.with_params(source.params().clone());
        if let Some((file, line)) = source.source() {
            meta = meta.defined_at(file, line);
        }
    }
    meta.register();
    patch_class(id);
    mark_live();
    // An alias is a definition: ruby reports the NEW name, once.
    fire_def_hook(DefTarget::Class(id), DefEvent::Added, new)?;
    Ok(RubyValue::Symbol(new))
}

/// `Module#private`/`public`/`protected` WITH NAME ARGUMENTS reached at
/// runtime (`Foo.class_eval { private :m }`, ostruct's guarded
/// `private :block_given!`): validate each name resolves as an instance
/// method of `id` (NameError otherwise, CRuby's timing) and record the mark
/// in the overlay, where `instance_method_visibility`'s walk finds it ahead
/// of the frozen registry's own flags. Returns the arguments as passed: a
/// lone name verbatim, several names as an Array (Ruby >= 3.1's shape).
///
/// The ARGUMENT-LESS form (set the default visibility for subsequent defs
/// in this scope) is accepted as a nil-returning no-op: the overlay has no
/// per-scope default, and compiled `def`s took their visibility at compile
/// time -- divergence limited to a bare `private` inside `class_eval`.
pub fn runtime_set_visibility(
    id: ClassId,
    args: &[RubyValue],
    vis: crate::dispatch::MethodVisibility,
) -> Result<RubyValue, Signal> {
    if args.is_empty() {
        // Bare `private` switches the enclosing body's default for every
        // subsequent `def`.
        update_frame_for(id, |f| f.vis = vis);
        // A compiled `class << self` body has no frame to record into --
        // the rebound directive persists as the SURROGATE's body default
        // (read by `runtime_define_method`'s singleton redirect), and the
        // lowering's body-end reset clears it.
        if current_frame_for(id).is_none() && singleton_owner_value(id).is_some() {
            let mut w = maps().classes.write().unwrap();
            w.entry(id.0)
                .or_insert_with(OverlayEntry::delta)
                .singleton_default_vis = Some(vis);
            drop(w);
            mark_live();
        }
        return Ok(RubyValue::Nil);
    }
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // `private [:a, :b]` (one Array argument) marks the contents. The return
    // value is the ARGUMENT SHAPE as passed -- a lone name (Symbol or
    // String) comes back verbatim, several names come back as an Array --
    // per CRuby's `Module#private` docs.
    let names: Vec<RubyValue> = match args {
        [RubyValue::Array(a)] => a.lock().to_vec(),
        one_or_many => one_or_many.to_vec(),
    };
    let result = match args {
        [one] => one.clone(),
        many => RubyValue::Array(crate::array_new(many.to_vec())),
    };
    let mut syms = Vec::with_capacity(names.len());
    for name in &names {
        syms.push(coerce_method_name(Some(name))?);
    }
    // A SINGLETON class's instance methods are its owner's CLASS methods --
    // `class << self; public(*METHODS); end` (fileutils' Verbose/NoWrite/
    // DryRun) is `public_class_method(*METHODS)` on the owner, and that is the
    // table those methods and their visibility actually live in. Without this
    // the names resolve against an instance table the singleton id never had.
    if let Some(owner) = singleton_class_owner(id) {
        for &sym in &syms {
            // Either provenance counts: a compiled `def self.x` (the owner's
            // class-method table) or one the singleton itself gained at run
            // time -- fileutils reaches this line right after `extend self`.
            let resolves = crate::dispatch::class_method_owner(owner, sym).is_some()
                || crate::dispatch::instance_method_visibility(id, sym).is_some()
                || snapshot_instance_method(id, sym).is_some();
            if !resolves {
                return Err(name_error!(
                    "undefined method '{}' for class '{}'",
                    sym.name(),
                    crate::dispatch::class_name(id).unwrap_or_default()
                ));
            }
        }
        let marks: Vec<RubyValue> = syms.iter().map(|&s| RubyValue::Symbol(s)).collect();
        runtime_class_method_visibility(
            owner,
            &marks,
            vis == crate::dispatch::MethodVisibility::Private,
        )?;
        return Ok(result);
    }
    for &sym in &syms {
        let resolves = crate::dispatch::instance_method_visibility(id, sym).is_some()
            || snapshot_instance_method(id, sym).is_some();
        if !resolves {
            let kind = if crate::dispatch::class_is_module(id).unwrap_or(false) {
                "module"
            } else {
                "class"
            };
            let cls = crate::dispatch::class_name(id).unwrap_or_default();
            return Err(name_error!(
                "undefined method '{}' for {kind} '{cls}'",
                sym.name()
            ));
        }
    }
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        for &sym in &syms {
            e.methods_vis.insert(sym, vis);
        }
    }
    patch_class(id);
    mark_live();
    // `private :m` on an INHERITED method is a definition -- CRuby synthesizes
    // a `ZSUPER` entry through `rb_add_method`, which fires the hook
    // (`vm_method.c:2318-2336`). On the class's OWN method the visibility is
    // set in place and nothing fires. Oracle-verified both ways.
    for &sym in &syms {
        if crate::dispatch::method_owner(id, sym) != Some(id) {
            fire_def_hook(DefTarget::Class(id), DefEvent::Added, sym)?;
        }
    }
    Ok(result)
}

/// The explicit runtime visibility mark for `name` on class `id`'s OWN
/// overlay entry -- `instance_method_visibility` probes this per ancestor,
/// nearest mark winning, ahead of the frozen registry's flags.
pub(crate) fn overlay_method_visibility(
    id: ClassId,
    name: Symbol,
) -> Option<crate::dispatch::MethodVisibility> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.methods_vis.get(&name).copied())
}

/// The CLASS a singleton-class id belongs to (`Foo.singleton_class` -> `Foo`),
/// or `None` for an ordinary id or an object's singleton. What lets the
/// singleton class's instance-method reflection answer from the owner's
/// CLASS-method tables, which is where `def self.x` really lives.
pub fn singleton_class_owner(id: ClassId) -> Option<ClassId> {
    match singleton_owner_value(id) {
        Some(RubyValue::Class(cid)) => Some(cid),
        _ => None,
    }
}

/// [`singleton_class_owner`] without the class narrowing -- the VALUE a
/// singleton-class id was minted for, whatever its kind.
pub fn singleton_owner_value(id: ClassId) -> Option<RubyValue> {
    maps().singleton_owner.read().unwrap().get(&id.0).cloned()
}

/// The singleton class ALREADY minted for `recv`, if any -- without minting
/// one, which is what makes it safe to ask from a reflection row. `def obj.x`
/// alone mints nothing; only naming `obj.singleton_class` does.
pub fn minted_singleton_class(recv: &RubyValue) -> Option<ClassId> {
    let key = singleton_class_key(recv)?;
    maps().singleton_classes.read().unwrap().get(&key).copied()
}

/// Whether a runtime `private_class_method`/`public_class_method` marked class
/// method `name` on `id`, and which way. `None` when neither was called for it.
pub(crate) fn overlay_class_method_private(id: ClassId, name: Symbol) -> Option<bool> {
    maps()
        .classes
        .read()
        .unwrap()
        .get(&id.0)
        .and_then(|e| e.class_methods_vis.get(&name).copied())
}

/// Retire `names` on `id` without the `undef_method` ceremony -- no
/// respond-to check, no `method_undefined` hook. Used where a class body that
/// WOULD have defined them never ran, so the definitions the compile-time
/// tables already carry have to be taken back. See
/// [`crate::dispatch::guard_class_reopen`].
pub(crate) fn retire_names(id: ClassId, names: &[&str]) {
    let mut w = maps().classes.write().unwrap();
    let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
    for n in names {
        let sym = Symbol::intern(n);
        e.undefs.insert(sym);
        e.methods.remove(&sym);
        e.class_methods.remove(&sym);
        e.class_undefs.insert(sym);
    }
    drop(w);
    patch_class(id);
    mark_live();
}

/// `private_class_method :x` / `public_class_method :x` at runtime -- see
/// [`overlay_class_method_private`].
pub fn runtime_class_method_visibility(
    id: ClassId,
    args: &[RubyValue],
    private: bool,
) -> Result<(), Signal> {
    let mut w = maps().classes.write().unwrap();
    let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
    for a in args {
        e.class_methods_vis
            .insert(coerce_method_name(Some(a))?, private);
    }
    drop(w);
    patch_class(id);
    mark_live();
    Ok(())
}

/// The `MethodImpl` that instance method `name` resolves to for instances of
/// `id`, in dispatch order per ancestor: overlay delta, registry method,
/// builtin-reopen value method, builtin table row -- the latter two wrapped
/// into the `MethodImpl` ABI (they run against a boxed receiver, so the
/// resulting alias serves `RObj` dispatch, which is where the overlay is
/// probed). A compile-time builtin-alias row resolves through its target
/// (rows are terminal, so the single recursion can't loop).
pub(crate) fn snapshot_instance_method(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    snapshot_from(id, name, false)
}

/// [`snapshot_instance_method`] with the overlay layer skipped on EVERY
/// ancestor -- the body that answered before any runtime `define_method` did.
///
/// This is how a `Method`/`UnboundMethod` re-finds the entry it froze. It
/// cannot simply keep the `MethodImpl`: zeo materializes a LAYOUT-CORRECT copy
/// of a compiled method per class, so `Foo#b`'s body cannot run against a `Sub`
/// instance. Freezing which LAYER answered, and re-resolving from that layer
/// for the actual receiver's class, is both redefinition-proof and
/// layout-correct.
pub(crate) fn snapshot_below_overlay(id: ClassId, name: Symbol) -> Option<MethodImpl> {
    snapshot_from(id, name, true)
}

fn snapshot_from(id: ClassId, name: Symbol, skip_overlay: bool) -> Option<MethodImpl> {
    let n = name.name();
    let n = n.as_str();
    for &anc in ancestors_of_value(id) {
        if !skip_overlay {
            let c = maps().classes.read().unwrap();
            if let Some(m) = c.get(&anc.0).and_then(|e| {
                e.prepended
                    .get(&name)
                    .or_else(|| e.methods.get(&name))
                    .cloned()
            }) {
                return Some(m);
            }
        }
        if let Some(m) = registry_lookup_cloned(anc, name) {
            return Some(m);
        }
        if let Some(m) = crate::dispatch::registry_value_method_impl(anc, name) {
            return Some(m);
        }
        if let Some(f) = crate::builtins::class_table(anc).and_then(|t| t(n)) {
            return Some(MethodImpl::Dynamic(Arc::new(
                move |recv: &RObj, args: &[RubyValue], block: Option<RubyValue>| {
                    f(&RubyValue::Object(recv.clone()), args, block)
                },
            )));
        }
    }
    crate::dispatch::alias_target(id, name).and_then(|old| snapshot_from(id, old, skip_overlay))
}

/// Whether `name` currently resolves for instances of `id` through the
/// OVERLAY -- a runtime `define_method` body. See [`snapshot_below_overlay`].
pub(crate) fn resolves_through_overlay(id: ClassId, name: Symbol) -> bool {
    if !is_live() {
        return false;
    }
    for &anc in ancestors_of_value(id) {
        {
            let c = maps().classes.read().unwrap();
            if c.get(&anc.0).is_some_and(|e| e.methods.contains_key(&name)) {
                return true;
            }
        }
        if registry_lookup_cloned(anc, name).is_some()
            || crate::dispatch::registry_value_method_impl(anc, name).is_some()
            || crate::builtins::class_table(anc).is_some_and(|t| t(name.name_str()).is_some())
        {
            return false;
        }
    }
    false
}

/// `Module#module_function(*names)` reached at RUNTIME -- fileutils calls
/// `module_function name` with a COMPUTED name inside its own
/// `private_module_function` helper (the plain literal form resolves at compile
/// time in `lower/defs.rs`). Promotes each named instance method to a
/// class/module method using the same wrapper `extend self` builds
/// (`extended_class_method`), so `Mod.name` and bare calls in class-method
/// context resolve. The instance copy stays -- that is the `include`-mixin half
/// of `module_function` -- but becomes PRIVATE, as in CRuby. The bare (no-arg)
/// mode form has no runtime spelling here and is a documented nil no-op.
pub fn runtime_module_function(id: ClassId, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if args.is_empty() {
        update_frame_for(id, |f| f.module_function = true);
        return Ok(RubyValue::Nil);
    }
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    let mut syms = Vec::with_capacity(args.len());
    for a in args {
        syms.push(coerce_method_name(Some(a))?);
    }
    // Build the wrappers BEFORE taking the overlay write lock:
    // `extended_class_method` itself reads the overlay (see `runtime_extend`).
    let mut installs = Vec::with_capacity(syms.len());
    for &sym in &syms {
        let proc_ = extended_class_method(id, sym).ok_or_else(|| {
            let cls = crate::dispatch::class_name(id).unwrap_or_default();
            name_error!("undefined method '{}' for module '{cls}'", sym.name())
        })?;
        installs.push((sym, proc_));
    }
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        for (sym, proc_) in installs {
            e.class_methods.insert(sym, proc_);
            e.extended_class_methods.remove(&sym);
            e.methods_vis
                .insert(sym, crate::dispatch::MethodVisibility::Private);
        }
    }
    patch_class(id);
    mark_live();
    // Only the module-method half is new here -- the instance copy already
    // existed and merely turned private, so ruby reports just the singleton.
    // (The BARE `module_function` mode reports both, from the `def` that
    // follows it; see `runtime_define_method`.)
    for &sym in &syms {
        fire_def_hook(
            DefTarget::Singleton(&RubyValue::Class(id)),
            DefEvent::Added,
            sym,
        )?;
    }
    Ok(match syms.as_slice() {
        [one] => RubyValue::Symbol(*one),
        many => RubyValue::Array(crate::array_new(
            many.iter().map(|s| RubyValue::Symbol(*s)).collect(),
        )),
    })
}

/// `define_method(name, method_obj)` -- install an instance method on `id`
/// whose body IS the `Method`/`UnboundMethod`'s source definition (`owner`
/// class, `src_name`). CRuby requires `id` be the source's owner or a
/// descendant (so `self` is a valid instance of the method's class),
/// TypeError otherwise. Snapshots the source impl (the same resolution
/// `alias_method` uses) and installs it under `name`. Returns the name.
pub fn runtime_define_method_from_method(
    id: ClassId,
    name: Symbol,
    owner: ClassId,
    src_name: Symbol,
) -> Result<RubyValue, Signal> {
    if crate::dispatch::class_frozen(id) {
        return Err(crate::dispatch::frozen_class_error(id));
    }
    // A MODULE-owned method binds anywhere (CRuby's rule -- rack installs
    // `ERB::Escape.instance_method(:html_escape)` into `Rack::Utils`, which
    // never includes it); only a CLASS-owned one requires the target to be
    // the owner or a descendant, so `self` is a valid instance.
    let owner_is_module = crate::dispatch::class_is_module(owner).unwrap_or(false);
    if !owner_is_module && !ancestors_of_value(id).contains(&owner) {
        return Err(type_error!(
            "bind argument must be a subclass of {}",
            crate::dispatch::class_name(owner).unwrap_or_default()
        ));
    }
    // Snapshot the method as resolved for the TARGET class's own instances,
    // not the owner's: zeo materializes each class's own layout-correct copy
    // of an inherited method, and the owner's compiled impl would panic on a
    // subclass-layout receiver. (For the common case where the target doesn't
    // override the name, this is the same behavior; a target that DOES
    // override it binds its own version -- a documented AOT divergence.)
    // A module owner outside the target's chain resolves nothing on the
    // target, so the module's own receiver-generic body answers instead.
    let m = snapshot_instance_method(id, src_name)
        .or_else(|| {
            owner_is_module
                .then(|| module_own_method_impl(owner, src_name))
                .flatten()
        })
        .ok_or_else(|| {
            name_error!(
                "undefined method '{}' for class '{}'",
                src_name.name(),
                crate::dispatch::class_name(owner).unwrap_or_default()
            )
        })?;
    // The VALUE-shaped copy as well, resolved in the same order `m` was. An
    // `RObj`-shaped `MethodImpl` has nothing to bind a Class or a builtin
    // receiver to, so without this the installed method is reachable only
    // from an ordinary OBJECT -- and both ways a module exposes itself
    // (`extend self`, `module_function`) call it with the MODULE as the
    // receiver.
    let value_body = crate::dispatch::value_method(id, 0, src_name)
        .or_else(|| crate::dispatch::value_method(owner, 0, src_name))
        .map(|f| RProc::with_self_and_block(f.into_fn(), RubyValue::Nil, -1, true));
    // A live `module_function` cursor governs this form exactly as it governs
    // a `def` or a `define_method` block: the instance copy turns PRIVATE and
    // a public module method appears beside it. rack's `Rack::Utils` opens
    // with a bare `module_function` and later installs `escape_html` from
    // `ERB::Escape.instance_method(:html_escape)`, so without this
    // `Rack::Utils.escape_html` was a NoMethodError while the instance copy
    // stayed wrongly public.
    let module_function = current_frame_for(id).is_some_and(|f| f.module_function);
    {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.methods.insert(name, m);
        e.undefs.remove(&name);
        e.removed.remove(&name);
        if let Some(vb) = value_body {
            e.value_bodies.insert(name, vb);
        }
        if module_function {
            e.methods_vis
                .insert(name, crate::dispatch::MethodVisibility::Private);
        } else {
            e.methods_vis.remove(&name);
        }
    }
    // Built with the overlay lock DROPPED: `extended_class_method` reads it.
    if module_function && let Some(wrapper) = extended_class_method(id, name) {
        let mut w = maps().classes.write().unwrap();
        let e = w.entry(id.0).or_insert_with(OverlayEntry::delta);
        e.class_methods.insert(name, wrapper);
        e.extended_class_methods.remove(&name);
    }
    patch_class(id);
    mark_live();
    fire_def_hook(DefTarget::Class(id), DefEvent::Added, name)?;
    // ruby reports BOTH halves: the instance copy, then the module copy.
    if module_function {
        fire_def_hook(
            DefTarget::Singleton(&RubyValue::Class(id)),
            DefEvent::Added,
            name,
        )?;
    }
    Ok(RubyValue::Symbol(name))
}

/// `recv.define_singleton_method(name, method_obj)` -- the method-object body
/// form: install the source method's definition as a per-object singleton (or
/// a class method for a `Class` receiver). The compatibility check runs
/// against the RECEIVER's class, matching CRuby's singleton bind rule.
pub fn runtime_define_singleton_from_method(
    recv: &RubyValue,
    name: Symbol,
    owner: ClassId,
    src_name: Symbol,
) -> Result<RubyValue, Signal> {
    let recv_class = recv.class_id();
    // Module-owned sources bind anywhere -- see
    // `runtime_define_method_from_method`.
    let owner_is_module = crate::dispatch::class_is_module(owner).unwrap_or(false);
    if !owner_is_module && !ancestors_of_value(recv_class).contains(&owner) {
        return Err(type_error!(
            "bind argument must be a subclass of {}",
            crate::dispatch::class_name(owner).unwrap_or_default()
        ));
    }
    // Snapshot as resolved for the RECEIVER's class (layout-correct copy) --
    // see the note in `runtime_define_method_from_method`.
    let m = snapshot_instance_method(recv_class, src_name)
        .or_else(|| {
            owner_is_module
                .then(|| module_own_method_impl(owner, src_name))
                .flatten()
        })
        .ok_or_else(|| {
            name_error!(
                "undefined method '{}' for class '{}'",
                src_name.name(),
                crate::dispatch::class_name(owner).unwrap_or_default()
            )
        })?;
    match recv {
        RubyValue::Class(cid) => {
            if crate::dispatch::class_frozen(*cid) {
                return Err(crate::dispatch::frozen_class_error(*cid));
            }
            // A class method whose body is a snapshot of an instance method:
            // wrap it so the `RubyValue::Class` receiver reaches the impl.
            let wrapped = RProc::with_self_and_block(
                move |self_val: &RubyValue, args: &[RubyValue], block: Option<RubyValue>| {
                    if let RubyValue::Object(o) = self_val {
                        m.call(o, args, block)
                    } else {
                        Err(type_error!(
                            "singleton method body needs an object receiver"
                        ))
                    }
                },
                recv.clone(),
                -1,
                true,
            );
            let mut w = maps().classes.write().unwrap();
            let e = w.entry(cid.0).or_insert_with(OverlayEntry::delta);
            e.class_methods.insert(name, wrapped);
            e.extended_class_methods.remove(&name);
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
        RubyValue::Object(o) => {
            crate::builtins::check_frozen(recv)?;
            let key = obj_identity(o);
            maps()
                .singletons
                .write()
                .unwrap()
                .entry(key)
                .or_default()
                .insert(name, m);
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
        // Only an ordinary object, unlike `runtime_define_singleton_method`
        // below: the body here is a snapshot of a compiled instance method,
        // whose signature takes an `RObj` receiver, so there is nothing to bind
        // a bare Array/String to.
        other => Err(type_error!(
            "can't define singleton method for {}",
            immediate_kind(other)
        )),
    }
}

/// `recv.define_singleton_method(name) { body }` -- a per-object singleton when
/// `recv` is an ordinary object, or a class/singleton method when `recv` is a
/// `Class`. A singleton on an immediate (Integer/Symbol/nil/...) is a
/// `TypeError`, matching CRuby.
pub fn runtime_define_singleton_method(
    recv: &RubyValue,
    name: Symbol,
    body: RProc,
) -> Result<RubyValue, Signal> {
    match recv {
        RubyValue::Class(cid) => {
            if crate::dispatch::class_frozen(*cid) {
                return Err(crate::dispatch::frozen_class_error(*cid));
            }
            crate::method_meta::record_runtime_params(
                *cid,
                crate::MethodKind::Singleton,
                name,
                &body,
            );
            {
                let mut w = maps().classes.write().unwrap();
                let e = w.entry(cid.0).or_insert_with(OverlayEntry::delta);
                e.class_methods.insert(name, body);
                e.extended_class_methods.remove(&name);
            }
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
        RubyValue::Object(o) => {
            // CRuby's rb_check_frozen on the singleton's attachee: a frozen
            // object refuses new singleton methods.
            crate::builtins::check_frozen(recv)?;
            let key = pin_identity(&RubyValue::Object(o.clone()))
                .expect("an Object always has an identity");
            crate::method_meta::record_singleton_params(key, name, &body);
            let m = dynamic_from_proc(SINGLETON_DEFINING, name, body);
            {
                let mut w = maps().singletons.write().unwrap();
                w.entry(key).or_default().insert(name, m);
            }
            clear_extended_name(key, name);
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
        other => {
            // Any other heap value (`def SOME_ARRAY.[](i)`) -- see
            // `value_identity`. The body stays an `RProc`, called with the value
            // itself as `self`, because a bare Array has no `RObj` to bind.
            let Some(key) = pin_identity(other) else {
                return Err(type_error!("can't define singleton"));
            };
            // `nil`/`true`/`false` report as frozen but still accept a
            // singleton -- CRuby's singleton class for them IS their class, and
            // takes no frozen check (oracle-verified). Every other value does.
            if !matches!(other, RubyValue::Nil | RubyValue::Bool(_)) {
                crate::builtins::check_frozen(recv)?;
            }
            crate::method_meta::record_singleton_params(key, name, &body);
            {
                let mut w = maps().value_singletons.write().unwrap();
                w.entry(key).or_default().insert(name, body);
            }
            clear_extended_name(key, name);
            mark_singletons();
            mark_live();
            // A singleton definition reports to the OBJECT, not to its
            // singleton class -- CRuby's `RCLASS_ATTACHED_OBJECT` rewrite.
            fire_def_hook(DefTarget::Singleton(recv), DefEvent::Added, name)?;
            Ok(RubyValue::Symbol(name))
        }
    }
}

/// `recv.extend(Mod)` -- mix a module's instance methods into the receiver's
/// singleton, so they resolve on `recv`. Works for every receiver kind Ruby
/// allows:
///
/// * an ORDINARY object -- methods land in the identity-keyed singleton table
///   `resolve_dynamic` consults first (`obj.foo` only for this object).
/// * a CLASS or MODULE (`SecureRandom.extend(Random::Formatter)`) -- methods
///   become the receiver's CLASS/module methods (its singleton class), so
///   `SecureRandom.hex` resolves. A native module's method runs with the Class
///   as `self` verbatim (redispatching e.g. `gen_random` back to the receiver);
///   a user module's method, whose compiled body wants an object receiver, runs
///   against a fresh instance of the receiver class.
///
/// An existing singleton/class method (`def self.x`) is never clobbered -- own
/// singletons outrank an extended module, exactly as in CRuby's ancestry.
///
/// `Object#extend` is DEFINED IN TERMS of `Module#extend_object` and does no
/// mixing itself (`eval.c`'s `rb_obj_extend`), the same shape `include` has
/// around `append_features`. A module that overrides the primitive therefore
/// controls what `extend` does to the receiver -- and an override that omits
/// `super` skips the mixin entirely while `extended` still fires. The default
/// primitive lands back in [`extend_object_default`], which is this function's
/// body minus the routing and the hook.
pub fn runtime_extend(recv: &RubyValue, module_val: &RubyValue) -> Result<RubyValue, Signal> {
    extend_object_or_primitive(recv, module_val)?;
    fire_mixin_hook(module_val, "extended", recv)?;
    Ok(recv.clone())
}

/// The mix-in half of [`runtime_extend`] with no notification: the module's
/// own `extend_object` when it overrides one, the default splice otherwise.
/// Shared with the singleton-class path, which sends a different hook.
fn extend_object_or_primitive(recv: &RubyValue, module_val: &RubyValue) -> Result<(), Signal> {
    let RubyValue::Class(mid) = module_val else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::check_type_name(module_val)
        ));
    };
    match overrides_mixin_primitive(*mid, "extend_object") {
        true => {
            crate::dispatch::send_value(
                module_val,
                Symbol::intern("extend_object"),
                std::slice::from_ref(recv),
                None,
            )?;
        }
        false => extend_object_default(recv, module_val)?,
    }
    Ok(())
}

/// `Module#extend_object`'s default body -- the mixin itself, without the
/// `extended` notification its caller owns. See [`runtime_extend`].
pub fn extend_object_default(recv: &RubyValue, module_val: &RubyValue) -> Result<(), Signal> {
    let RubyValue::Class(mid) = module_val else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::check_type_name(module_val)
        ));
    };
    // A repeat `extend` re-ranks nothing: the module keeps the position its
    // FIRST one gave it, so re-copying its methods would wrongly promote it
    // over a module extended in between. The `extended` hook still fires each
    // time -- which it does, because the caller owns it (both oracle-verified).
    //
    // A module already PREPENDED into this singleton counts as present, and
    // that is the whole of what makes document order decide. `rb_include_
    // module` searches the whole chain (`search_super = TRUE`), so an extend
    // after a prepend finds it and adds nothing; `rb_prepend_module` searches
    // only the prepend area, so a prepend after an extend adds a SECOND
    // position. Oracle-verified in all four orders.
    if extended_modules(recv).contains(mid)
        || matches!(recv, RubyValue::Class(cid)
            if resolver::singleton_prepends_of(*cid).contains(mid))
    {
        return Ok(());
    }
    let names = module_extendable_method_names(*mid);
    match recv {
        RubyValue::Object(o) => {
            // CRuby's rb_check_frozen: a frozen object refuses `extend` (its
            // singleton table is what would change).
            crate::builtins::check_frozen(recv)?;
            let key = pin_identity(&RubyValue::Object(o.clone()))
                .expect("an Object always has an identity");
            let mut copied = Vec::new();
            {
                let mut w = maps().singletons.write().unwrap();
                let table = w.entry(key).or_default();
                for name in names {
                    if let Some(m) = module_own_method_impl(*mid, name) {
                        table.insert(name, m);
                        copied.push(name);
                    }
                }
            }
            record_extended_names(key, *mid, &copied);
        }
        RubyValue::Class(cid) => {
            if crate::dispatch::class_frozen(*cid) {
                return Err(crate::dispatch::frozen_class_error(*cid));
            }
            // Build every wrapper BEFORE taking the overlay write lock:
            // `extended_class_method` reads the overlay (module_own_method_impl),
            // so building under the lock would deadlock.
            let installs: Vec<(Symbol, RProc)> = names
                .into_iter()
                // Own `def self.x` (materialized into the registry) outranks the
                // module, so leave it be -- also what keeps SecureRandom's own
                // `gen_random` from being shadowed by the mixin's bridge copy.
                .filter(|&name| !crate::dispatch::class_defines_own_class_method(*cid, name))
                .filter_map(|name| extended_class_method(*mid, name).map(|p| (name, p)))
                .collect();
            let mut w = maps().classes.write().unwrap();
            let entry = w.entry(cid.0).or_insert_with(OverlayEntry::delta);
            for (name, proc_) in installs {
                // A later `extend` layers ABOVE an earlier one (CRuby ancestry),
                // so it wins on a name collision between two mixins.
                entry.class_methods.insert(name, proc_);
                entry.extended_class_methods.insert(name);
            }
        }
        other => {
            // Any other HEAP value (`ARGV.extend(OptionParser::Arguable)`, the
            // last line of optparse) -- the same value-keyed singleton table
            // `def SOME_ARRAY.m` writes to, since a bare Array has no `RObj` to
            // bind a compiled body against. Immediates have no singleton
            // storage here at all, same posture as `define_singleton_method`.
            // An Integer/Float/Symbol has no singleton storage in CRuby either,
            // and reports it with the same wording `define_singleton_method`
            // does -- not a per-kind message (oracle-verified). `nil`/`true`/
            // `false` DO accept one, and take no frozen check.
            let Some(key) = pin_identity(other) else {
                return Err(type_error!("can't define singleton"));
            };
            if !matches!(other, RubyValue::Nil | RubyValue::Bool(_)) {
                crate::builtins::check_frozen(recv)?;
            }
            // Built before the write lock, like the Class arm above:
            // `extended_value_method` reads the overlay.
            let installs: Vec<(Symbol, RProc)> = names
                .into_iter()
                .filter_map(|name| extended_value_method(*mid, name).map(|p| (name, p)))
                .collect();
            let copied: Vec<Symbol> = installs.iter().map(|(n, _)| *n).collect();
            {
                let mut w = maps().value_singletons.write().unwrap();
                let table = w.entry(key).or_default();
                for (name, proc_) in installs {
                    table.insert(name, proc_);
                }
            }
            record_extended_names(key, *mid, &copied);
        }
    }
    // The method copies above make the module ANSWER on `recv`; this is what
    // makes `recv` BE one -- `is_a?`, `===` and `singleton_class.ancestors`
    // all read it, and none of them can see a copied method table.
    record_extended(recv, *mid);
    refresh_singleton_ancestors(recv);
    mark_singletons();
    mark_ancestry_mutated();
    mark_live();
    Ok(())
}

/// `K.singleton_class.prepend(M)` -- M's instance methods become K's CLASS
/// methods at HIGHER priority than K's own `def self.x`, with `super` from one
/// resuming at the shadowed definition (the ForkTracker / fork-hook shape,
/// reached at runtime when K itself is a runtime-minted class the compile-time
/// ancestry edit could not name). The copies live in their own layer
/// (`prepended_class_methods`), probed before everything by
/// [`overlay_class_method`] and skipped when a prepended method's own `super`
/// resumes the walk (`send_super_class_from`).
///
/// Reflection nuance, written down rather than papered over (the posture of
/// the singleton-include note in [`mix_in`]): the module is recorded via the
/// extended list, so `K.singleton_class.ancestors` reports it AFTER the
/// singleton head where CRuby puts it before.
fn prepend_into_class_singleton(owner: ClassId, module_val: &RubyValue) -> Result<(), Signal> {
    let RubyValue::Class(mid) = module_val else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::check_type_name(module_val)
        ));
    };
    if crate::dispatch::class_frozen(owner) {
        return Err(crate::dispatch::frozen_class_error(owner));
    }
    // Re-prepending a module already in the stack is a no-op, as CRuby's is
    // -- and overwriting the winner copies would silently hoist it above
    // later prepends.
    if has_singleton_prepend(owner, *mid) {
        return Ok(());
    }
    // Built before the write lock, like `extend_object_default`'s Class arm:
    // `extended_class_method` reads the overlay.
    let installs: Vec<(Symbol, RProc)> = module_extendable_method_names(*mid)
        .into_iter()
        .filter_map(|name| extended_class_method(*mid, name).map(|p| (name, p)))
        .collect();
    let owner_val = RubyValue::Class(owner);
    {
        let mut w = maps().classes.write().unwrap();
        let entry = w.entry(owner.0).or_insert_with(OverlayEntry::delta);
        for (name, proc_) in installs {
            // A later prepend layers ABOVE an earlier one (CRuby ancestry), so
            // it wins a name collision -- the extend table's rule. The shadowed
            // copy stays reachable: a prepended method's `super` resumes down
            // the recorded stack (`singleton_prepend_super_below`).
            entry.prepended_class_methods.insert(name, proc_);
        }
        entry.singleton_prepends.push(*mid);
    }
    // A prepend is NOT recorded as an extend. It used to be, so that `is_a?`
    // and `singleton_class.ancestors` would see it -- and that is exactly what
    // made a module reached BOTH ways (`extend M; singleton_class.prepend M`)
    // indistinguishable from one reached only by prepending. Ruby gives the
    // first TWO chain positions and the second one, so the two verbs are
    // recorded apart now: `singleton_prepends` above for the prepend area,
    // the extended list for the include side. `value_extends` reads both.
    //
    // The gate still arms: it says "some singleton chain has been mixed
    // into", and the readers behind it must run.
    GATES.fetch_or(GATE_ANY_EXTENDED, std::sync::atomic::Ordering::Release);
    refresh_singleton_ancestors(&owner_val);
    mark_singletons();
    mark_ancestry_mutated();
    mark_live();
    Ok(())
}

/// `Module#include(M, ...)` reached AT RUNTIME on a Class/Module receiver --
/// e.g. `Class.new { include M }`. Splices each module (and its own ancestors
/// not already present) into the receiver's overlay ancestry right after the
/// receiver itself, matching CRuby's insertion point, so instances dispatch the
/// module's methods through `dispatch::send_in`'s ancestor walk (which finds a
/// compiled module's `emit_user_module_bridges` value-methods by id, and a
/// runtime `Module.new`'s overlay methods).
///
/// Only effective for a receiver whose ancestry lives in the overlay (a runtime
/// `Class.new`/`Module.new`); a FROZEN compiled class's registry ancestry is
/// immutable, so a runtime `include` on it is a documented no-op on dispatch
/// (rare -- static `include` is the compiled path).
pub fn runtime_include(recv: &RubyValue, modules: &[RubyValue]) -> Result<RubyValue, Signal> {
    // `include A, B` inserts each right after self, so the LAST argument ends
    // up closest to self -- process right-to-left to reproduce that order.
    mix_in(recv, modules, Placement::After, "include")
}

/// `Module#prepend(M, ...)` -- the mirror of `include`, splicing before the
/// receiver so the module's methods win over the receiver's own. `super` from
/// one then resumes at the receiver, because `send_super_from` walks by
/// POSITION and the module's id is what was pushed as the defining class.
pub fn runtime_prepend(recv: &RubyValue, modules: &[RubyValue]) -> Result<RubyValue, Signal> {
    mix_in(recv, modules, Placement::Before, "prepend")
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Placement {
    Before,
    After,
}

fn mix_in(
    recv: &RubyValue,
    modules: &[RubyValue],
    placement: Placement,
    verb: &str,
) -> Result<RubyValue, Signal> {
    let RubyValue::Class(cid) = recv else {
        return Err(type_error!("can't {verb} into {}", immediate_kind(recv)));
    };
    // `obj.singleton_class.include(M)` IS `obj.extend(M)` -- CRuby DEFINES the
    // latter as the former (`rb_include_module(rb_singleton_class(obj), M)`),
    // so the primitive and the wrapper are one operation. zeo implements a
    // singleton as a copied method table (`extend_object_default`) rather than
    // spliced ancestry, so splicing this id would write somewhere the owner's
    // dispatch never reads: the mixin would vanish silently.
    //
    // rdoc's `class << self; prepend Git`, spreadsheet's
    // `class << self; include Compatibility` and treetop's are all this shape.
    //
    // `prepend` into a CLASS's singleton has machinery of its own: the module
    // must outrank the owner's `def self.x`, which the extend table (own
    // definitions win there) cannot express. On a plain OBJECT's singleton,
    // `prepend` still lands in the extend table, which is only observable
    // against the object's OWN `def obj.x` -- CRuby would let the module win
    // over that, and this does not. Written down rather than papered over; no
    // gem in the corpus depends on the difference.
    if let Some(owner) = singleton_owner_value(*cid) {
        for module_val in modules {
            match (&owner, placement) {
                (RubyValue::Class(owner_id), Placement::Before) => {
                    prepend_into_class_singleton(*owner_id, module_val)?;
                    fire_mixin_hook(module_val, "prepended", recv)?;
                }
                // The INSTALL is the extend table either way (that is what a
                // zeo singleton is), but the NOTIFICATION is the one ruby
                // sends for the verb that was written, with the singleton
                // CLASS as its argument -- `singleton_class.include M` fires
                // `M.included(#<Class:K>)`, not `M.extended(K)`.
                _ => {
                    extend_object_or_primitive(&owner, module_val)?;
                    let hook = match placement {
                        Placement::Before => "prepended",
                        Placement::After => "included",
                    };
                    fire_mixin_hook(module_val, hook, recv)?;
                }
            }
        }
        return Ok(recv.clone());
    }
    if crate::dispatch::class_frozen(*cid) {
        return Err(crate::dispatch::frozen_class_error(*cid));
    }
    // ONE multi-argument call keeps its arguments in source order --
    // `include A, B` is `[self, A, B]` and `prepend A, B` is `[A, B, self]`
    // -- while separate calls put the LATEST closest to self. Each splice
    // lands its module closest to self, so both walk the arguments in
    // reverse to leave the first one outermost.
    let ordered: Vec<&RubyValue> = modules.iter().rev().collect();
    let (primitive, hook) = match placement {
        Placement::Before => ("prepend_features", "prepended"),
        Placement::After => ("append_features", "included"),
    };
    // BEFORE the loop: each splice reads the chain the previous one wrote,
    // and an overlay chain is only consulted once the overlay is live. A
    // multi-argument call spliced every module against the FROZEN chain
    // otherwise, so the last write won and the rest vanished.
    mark_ancestry_mutated();
    mark_live();
    for module_val in ordered {
        let RubyValue::Class(mid) = module_val else {
            return Err(type_error!(
                "wrong argument type {} (expected Module)",
                crate::builtins::check_type_name(module_val)
            ));
        };
        // `Module#include` is DEFINED IN TERMS of `append_features` and does no
        // splicing itself (`eval.c`'s `rb_mod_include`), which is what lets a
        // module police how it is mixed in -- the `singleton` gem overrides
        // this very method to reject inclusion into a module. An override that
        // omits `super` therefore skips the mixin entirely, and `included`
        // still fires. Both oracle-verified.
        match overrides_mixin_primitive(*mid, primitive) {
            true => {
                crate::dispatch::send_value(
                    module_val,
                    Symbol::intern(primitive),
                    std::slice::from_ref(recv),
                    None,
                )?;
            }
            false => {
                splice_module_into(*cid, *mid, placement);
                // The same overlay copy `Module#prepend_features` makes: a
                // prepend has to outrank the target's OWN methods, and the
                // flattened per-class table wins over the spliced chain --
                // see `overlay_prepended_methods`.
                if placement == Placement::Before {
                    overlay_prepended_methods(*cid, *mid);
                }
            }
        }
        fire_mixin_hook(module_val, hook, recv)?;
    }
    mark_ancestry_mutated();
    patch_class(*cid);
    mark_live();
    Ok(recv.clone())
}

/// Whether `module` supplies its own body for one of the three mix-in
/// PRIMITIVES, as opposed to inheriting `Module`'s. The defaults are the splice
/// itself, so reaching them through a send would be an infinite regress --
/// `Module#append_features` calls the splice directly for exactly that reason.
pub(crate) fn overrides_mixin_primitive(module: ClassId, primitive: &str) -> bool {
    crate::dispatch::class_method_owner(module, Symbol::intern(primitive)).is_some()
}

/// The splice `Module#prepend_features`/`#append_features` performs -- the
/// primitive, with no notification hook of its own.
pub fn splice_mixin(target: &RubyValue, module: &RubyValue, before: bool) -> Result<(), Signal> {
    let (RubyValue::Class(cid), RubyValue::Class(mid)) = (target, module) else {
        return Err(type_error!(
            "wrong argument type {} (expected Module)",
            crate::builtins::check_type_name(target)
        ));
    };
    if crate::dispatch::class_frozen(*cid) {
        return Err(crate::dispatch::frozen_class_error(*cid));
    }
    let placement = match before {
        true => Placement::Before,
        false => Placement::After,
    };
    splice_module_into(*cid, *mid, placement);
    if before {
        overlay_prepended_methods(*cid, *mid);
    }
    mark_ancestry_mutated();
    patch_class(*cid);
    mark_live();
    Ok(())
}

/// A PREPEND has to outrank the target's OWN methods, and `send_in` resolves an
/// object through a flattened per-class table before it walks any ancestry --
/// so splicing the chain alone leaves the class's own body still winning.
///
/// Copy the module's methods into the target's overlay, which IS probed first.
/// The copies come from the module's value-method container, whose bodies take
/// their receiver as an argument, so one copy serves every instance -- unlike a
/// compiled class method, which is laid out per class. Only the module's OWN
/// methods; one it inherited in turn stays on the ancestry walk.
fn overlay_prepended_methods(cid: ClassId, mid: ClassId) {
    let names = crate::dispatch::instance_method_names(mid, crate::dispatch::VisFilter::All, false);
    let installs: Vec<(Symbol, MethodImpl)> = names
        .into_iter()
        .filter_map(|n| crate::dispatch::registry_value_method_impl(mid, n).map(|m| (n, m)))
        .collect();
    if installs.is_empty() {
        return;
    }
    let mut w = maps().classes.write().unwrap();
    let e = w.entry(cid.0).or_insert_with(OverlayEntry::delta);
    for (name, m) in installs {
        e.prepended.insert(name, m);
    }
}

/// Ruby's mixin hook, run right after the ancestry edit: `M.included(target)`,
/// `M.extended(target)`, `M.prepended(target)`. `Module`'s own default is a
/// no-op, so nothing is dispatched unless the module really defines one.
pub(crate) fn fire_mixin_hook(
    module: &RubyValue,
    hook: &str,
    target: &RubyValue,
) -> Result<(), Signal> {
    let RubyValue::Class(mid) = module else {
        return Ok(());
    };
    let sym = Symbol::intern(hook);
    // A module minted by `class X < Module` reaches the hook as an INSTANCE
    // method of X -- CRuby looks it up through the module's singleton chain,
    // which runs into its class. `class_method_owner` only sees class methods
    // of the module itself, so it misses that one; Rails' `included` hooks on
    // `DeprecatedConstantProxy` are exactly this shape.
    let defined = crate::dispatch::class_method_owner(*mid, sym).is_some()
        || module_owner_class(*mid).is_some_and(|owner| module_subclass_defines(owner, sym));
    if !defined {
        return Ok(());
    }
    crate::dispatch::send_value(module, sym, std::slice::from_ref(target), None)?;
    Ok(())
}

/// Which of Ruby's three definition events happened. The hook's NAME is this
/// plus the target's shape: the same event is `method_added` on a class and
/// `singleton_method_added` on a singleton.
#[derive(Clone, Copy)]
pub(crate) enum DefEvent {
    Added,
    Removed,
    Undefined,
}

/// Where a definition landed. CRuby reads this off the target class's
/// `RCLASS_SINGLETON_P` bit and then rewrites the receiver to the attached
/// object (`vm_method.c`'s `CALL_METHOD_HOOK`). zeo's runtime writers already
/// know which of the two they are -- a singleton definition never reaches a
/// shared class's method table -- so they say so rather than making this
/// re-derive it from an id.
pub(crate) enum DefTarget<'a> {
    Class(ClassId),
    Singleton(&'a RubyValue),
}

/// Hook names reopened onto `Module`/`Class`/`BasicObject`, which apply to
/// EVERY class in the program. No per-class owner scan can see one: the reopen
/// registers an ordinary instance method whose owner id is the very id the
/// no-op default carries, so the two are indistinguishable by owner. Recorded
/// explicitly instead, by whoever installs it.
static ANY_GLOBAL_DEF_HOOK: AtomicBool = AtomicBool::new(false);
static GLOBAL_DEF_HOOKS: OnceLock<RwLock<FSet<Symbol>>> = OnceLock::new();

/// `hook` was defined on `Module`/`Class`/`BasicObject` itself. Called by
/// codegen for a compiled reopen and by [`runtime_define_method`] for a
/// runtime one.
pub fn mark_global_def_hook(hook: &str) {
    GLOBAL_DEF_HOOKS
        .get_or_init(|| RwLock::new(FSet::default()))
        .write()
        .unwrap()
        .insert(Symbol::intern(hook));
    ANY_GLOBAL_DEF_HOOK.store(true, Ordering::Release);
}

pub(super) fn global_def_hook(hook: Symbol) -> bool {
    ANY_GLOBAL_DEF_HOOK.load(Ordering::Acquire)
        && GLOBAL_DEF_HOOKS
            .get()
            .is_some_and(|h| h.read().unwrap().contains(&hook))
}

/// Whether a body the USER wrote will answer `hook`, as opposed to the no-op
/// default. Every caller reaches this from a runtime definition, which already
/// pays a write lock and an O(#classes) `patch_class`, so an MRO scan here
/// costs nothing worth latching around.
fn def_hook_runs(target: &DefTarget<'_>, hook: Symbol) -> bool {
    if global_def_hook(hook) {
        return true;
    }
    match target {
        // `class_method_owner` finds a `def self.method_added` and an
        // `extend`ed module's copy of one, and deliberately does NOT find
        // `Module`'s own no-op row: that is an INSTANCE method of Module, and
        // this scan walks class-method tables. So the default costs nothing.
        DefTarget::Class(cid) | DefTarget::Singleton(RubyValue::Class(cid)) => {
            crate::dispatch::class_method_owner(*cid, hook).is_some()
        }
        // `singleton_method_added` IS an instance method of `BasicObject`, so
        // here the scan does reach the default and it has to be ruled out by
        // owner. `Numeric`'s override -- which refuses a singleton on a number
        // at all -- resolves to `Numeric` and passes.
        DefTarget::Singleton(v) => {
            singleton_method_names(v).contains(&hook)
                || crate::dispatch::method_owner(v.class_id(), hook)
                    .is_some_and(|owner| owner != zeo_abi::BASIC_OBJECT_CLASS)
        }
    }
}

/// Ruby's definition hook: `Klass.method_added(:name)`, or its singleton twin
/// on the attached object. Run right after the definition lands, which is
/// CRuby's order -- the method is already callable when the hook sees its name.
///
/// Call this with every overlay lock DROPPED. A hook body that defines another
/// method is the whole point of the hook, and it takes the write lock again.
pub(crate) fn fire_def_hook(
    target: DefTarget<'_>,
    event: DefEvent,
    name: Symbol,
) -> Result<(), Signal> {
    let hook = match (&target, event) {
        (DefTarget::Class(_), DefEvent::Added) => "method_added",
        (DefTarget::Class(_), DefEvent::Removed) => "method_removed",
        (DefTarget::Class(_), DefEvent::Undefined) => "method_undefined",
        (DefTarget::Singleton(_), DefEvent::Added) => "singleton_method_added",
        (DefTarget::Singleton(_), DefEvent::Removed) => "singleton_method_removed",
        (DefTarget::Singleton(_), DefEvent::Undefined) => "singleton_method_undefined",
    };
    let hook = Symbol::intern(hook);
    if !def_hook_runs(&target, hook) {
        return Ok(());
    }
    let recv = match target {
        DefTarget::Class(cid) => RubyValue::Class(cid),
        DefTarget::Singleton(v) => v.clone(),
    };
    // `send_value`, not a visibility-checked call: CRuby reaches its hooks
    // through `rb_funcallv`, an FCALL, so a `private def self.method_added`
    // still runs.
    crate::dispatch::send_value(&recv, hook, &[RubyValue::Symbol(name)], None)?;
    Ok(())
}
