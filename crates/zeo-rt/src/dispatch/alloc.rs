//! Allocation and construction through the registry: allocators,
//! constructors, `construct_by_class_id`, and the `const_missing` hook
//! probe.

use super::*;

/// Whether `cid`'s chain defines a `const_missing` of its OWN -- anything
/// but `Module`'s default row, which only raises. A miss dispatches the
/// hook where one exists and takes the caller's pre-qualified NameError
/// where none does, because the default row rebuilds the message from the
/// receiver and a cref-qualified miss (`#<Class:Host>::X`) is not a
/// receiver's to name.
#[must_use]
pub fn user_const_missing(cid: ClassId) -> bool {
    class_method_owner(cid, Symbol::intern("const_missing"))
        .is_some_and(|owner| owner != zeo_abi::MODULE_CLASS)
}

/// A constant MISS dispatches `const_missing` on the owning class -- a user
/// hook (compiled `def self.const_missing`, or one defined at runtime)
/// answers; Module's default row raises the same qualified NameError the
/// miss would have. CRuby's exact protocol, which nothing in this runtime
/// called before: the hook had a definition but no caller.
pub fn const_miss(cid: ClassId, name: &str) -> Result<RubyValue, Signal> {
    // An `autoload` gets its one chance HERE, before the miss is reported.
    // Reading the constant is what runs it in ruby, and this is the read.
    if let Some(v) = crate::builtins::rmodule::run_autoload_for(cid, name)? {
        return Ok(v);
    }
    send_value(
        &RubyValue::Class(cid),
        Symbol::intern("const_missing"),
        &[RubyValue::Symbol(Symbol::intern(name))],
        None,
    )
}

/// The `AllocatorFn` `id` itself registered -- only `ruby_class!`-generated
/// (compiled user) classes register one, so a `Some` here identifies a class
/// whose instances are real generated structs.
pub(crate) fn registry_allocator(id: ClassId) -> Option<AllocatorFn> {
    REGISTRY.get()?.entries.get(&id.0).and_then(|e| e.allocator)
}

/// The nearest ancestor of `id` (per the live chain, self included) that
/// registered an `AllocatorFn` -- how a runtime subclass of a COMPILED class
/// allocates instances that inherited compiled methods can downcast: the
/// ancestor's struct, stamped with the SUBCLASS's id (see `ruby_class!`'s
/// `__class` field).
pub(crate) fn ancestor_allocator_of(id: ClassId) -> Option<AllocatorFn> {
    // A `Class#dup` copy's chain deliberately does NOT contain its source
    // (`K.dup.ancestors` skips `K`, as ruby's does), so the compiled struct
    // its copied bodies downcast to is reachable only through the allocator
    // the copy recorded for itself. See `OverlayEntry::allocator`.
    crate::runtime_meta::overlay_allocator(id).or_else(|| {
        ancestors_of_value(id)
            .iter()
            .find_map(|a| registry_allocator(*a))
    })
}

/// `id`'s OWN registered class-method body (`def self.x`), as the raw
/// implementation -- what `Class#dup` copies onto the new class.
/// Deliberately not the flattened probe: an INHERITED class method belongs
/// to the ancestor, and the copy keeps the same ancestor.
pub(crate) fn own_class_method_fn(id: ClassId, name: Symbol) -> Option<ValueImpl> {
    REGISTRY
        .get()?
        .entries
        .get(&id.0)?
        .class_methods
        .get(&(crate::boxes::current_box(), name))
        .copied()
}

/// The registry's dynamic constructor for `id` (`Class#new`'s row) --
/// `None` for modules, builtins without allocators, or a missing registry.
pub(crate) fn constructor_of(id: ClassId) -> Option<ConstructorFn> {
    // A C extension's `rb_define_alloc_func` wins, for the reason it wins in
    // [`allocate_of`]: it is what makes `Foo.new` produce a TypedData rather
    // than a plain object, and the C `initialize` reads that payload with
    // `DATA_PTR` on its first line.
    //
    // It has to be asked HERE, per call, and not once when the class was
    // minted. `rb_define_class` creates the class and `rb_define_alloc_func`
    // runs later in the same `Init_`, so a runtime class has already recorded
    // a `DynObject` constructor by then -- which is why `Foo.allocate` (which
    // asks `allocate_of`) worked while `Foo.new` handed the C `initialize` an
    // object carrying no struct. `zeo_rt_class_new_instance` intercepts the
    // COMPILED half for the same reason and says so in the same words.
    if crate::capi_hooks::has_alloc_func(id) {
        return Some(construct_by_c_allocator);
    }
    if let Some(c) = REGISTRY
        .get()
        .and_then(|r| r.entries.get(&id.0))
        .and_then(|e| e.constructor)
    {
        return Some(c);
    }
    // A runtime class (`Class.new`) registers its generic constructor in the
    // overlay, so `RuntimeClass.new` flows through the same `Class#new` ->
    // `construct_by_class_id` path frozen classes use.
    if crate::runtime_meta::is_live() {
        return crate::runtime_meta::overlay_constructor(id);
    }
    None
}

/// A fresh uninitialized instance of `id` (`Class#allocate`) -- a user class's
/// registered allocator, or a runtime (`Class.new`) class's name-keyed object.
/// `None` for modules/builtins (handled at the `allocate` call site).
pub(crate) fn allocate_of(id: ClassId) -> Option<RubyValue> {
    // A C extension's `rb_define_alloc_func` wins, and has to: it is what
    // makes `Foo.new` produce a TypedData rather than a plain object, and
    // every later `RTYPEDDATA_DATA` on the result depends on it. Checking it
    // FIRST is also what CRuby does -- the C allocator replaces whatever the
    // class had.
    if let Some(v) = crate::capi_hooks::c_allocate(id) {
        return Some(v);
    }
    if let Some(v) = REGISTRY.get().and_then(|r| r.allocate_instance(id)) {
        return Some(v);
    }
    if crate::runtime_meta::is_live() {
        return crate::runtime_meta::runtime_allocate(id);
    }
    None
}

/// `Class#new` for a class whose allocator came from a C extension: run that
/// allocator, then `initialize` on what it produced.
///
/// CRuby's `rb_class_new_instance` in two lines. The allocator is what puts
/// the struct behind `DATA_PTR`, so the order is not negotiable -- a C
/// `initialize` writes through that pointer immediately.
fn construct_by_c_allocator(
    id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // `c_allocate` answers `None` for a RAISE from inside the allocator, and
    // parks the signal -- `allocate_of` returns an Option, so that is the
    // only way one travels. Here there is a `Result` to put it in.
    let Some(obj) = crate::capi_hooks::c_allocate(id) else {
        return Err(crate::signal::take_pending().unwrap_or_else(|| {
            crate::builtins::type_error!(
                "allocator undefined for {}",
                crate::dispatch::class_name(id).unwrap_or_else(|| "Class".into())
            )
        }));
    };
    crate::dispatch::send_value(&obj, Symbol::intern("initialize"), args, block)?;
    Ok(obj)
}

/// Construct an instance of the class with id `id`, running its `initialize`.
/// Codegen calls this at every raise/construct site for a BOOTSTRAP exception
/// class, since those classes have no generated Rust struct to name --
/// the native exceptions registered their `ConstructorFn` (see
/// `crate::builtins::exception`).
/// A missing constructor is a zeo bug (a bootstrap id with no registrar).
pub fn construct_by_class_id(
    id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match constructor_of(id) {
        Some(ctor) => ctor(id, args, block),
        None => panic!("no constructor registered for class id {}", id.0),
    }
}
