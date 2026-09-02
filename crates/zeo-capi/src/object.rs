//! Objects, classes, modules, constants and variables.
//!
//! Almost every entry here is a C name for something zeo's `dispatch` and
//! `runtime_meta` already do, so almost every one is a call rather than a
//! reimplementation -- the same argument [`super::forward`] makes. What
//! separates these from a forwarded row is that the C signature is not
//! `VALUE (VALUE, ...)`: an `ID`, a `const char *`, an `int` flag or an
//! `argc`/`argv` pair has to be decoded first, and the decode is the whole
//! content of most of these functions.
//!
//! # The three constant lookups are three different searches
//!
//! MRI spells them apart and an extension picks deliberately, so zeo does
//! too:
//!
//! | Entry | Searches |
//! |---|---|
//! | `rb_const_get_at` | the module ITSELF, and nothing else |
//! | `rb_const_get_from` | the module and its ancestors, but NOT `Object` |
//! | `rb_const_get` | the module, its ancestors, and `Object` |
//!
//! The difference is visible: `Foo::Errno` raises where `Foo.const_get(:Errno)`
//! answers, and that is the `exclude` flag zeo's `const_get_scoped` carries.

use super::convert::{to_value, value_of};
use super::symbol::{Id, symbol_of};
use super::value::{self, Value};
use std::ffi::{c_char, c_int, c_long};
use zeo_rt::builtins::wrong_arg_type;
use zeo_rt::dispatch::ClassId;
use zeo_rt::{RubyValue, Signal, Symbol};

/// A C extension loads into the main program, so its globals are the main
/// program's. A box gets its own extension only when G7 B2 lands.
const MAIN_BOX: u32 = 0;

/// # Safety
///
/// `p` must be NUL-terminated, or null.
pub(super) unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract.
    unsafe { String::from_utf8_lossy(std::ffi::CStr::from_ptr(p).to_bytes()).into_owned() }
}

/// # Safety
///
/// `v` must be a live `VALUE`.
pub(super) unsafe fn as_class(v: Value) -> Result<ClassId, Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Class(cid) => Ok(cid),
        other => Err(wrong_arg_type(&other, "Class/Module")),
    }
}

/// Call `meth` on `recv`, through the same door Ruby code uses.
pub(super) fn send(recv: &RubyValue, meth: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    zeo_rt::dispatch::send_value(recv, Symbol::intern(meth), args, None)
}

/// # Safety
///
/// `argv` must name `argc` readable `VALUE`s.
pub(super) unsafe fn args_of(argc: c_int, argv: *const Value) -> Vec<RubyValue> {
    let n = argc.max(0) as usize;
    if n == 0 || argv.is_null() {
        return Vec::new();
    }
    // SAFETY: the caller's contract.
    unsafe { std::slice::from_raw_parts(argv, n) }
        .iter()
        .map(|v| unsafe { value_of(*v) })
        .collect()
}

/// `send` with an `argc`/`argv` pair, which is how MRI spells a Ruby method
/// that takes a variable number of arguments.
///
/// # Safety
///
/// `argv` must name `argc` readable `VALUE`s.
unsafe fn send_argv(
    recv: Value,
    meth: &str,
    argc: c_int,
    argv: *const Value,
) -> Result<Value, Signal> {
    let recv = unsafe { value_of(recv) };
    let args = unsafe { args_of(argc, argv) };
    to_value(&send(&recv, meth, &args)?)
}

/// `Object`'s own `const_get`, which is the search `rb_const_get` names.
fn const_lookup(owner: ClassId, name: &str, scope: Scope) -> Option<RubyValue> {
    match scope {
        Scope::Own => zeo_rt::constants::const_get_own(owner.0, name),
        // `const_get_scoped` is `::`'s search: ancestors, and NOT Object.
        Scope::From => zeo_rt::constants::const_get_scoped(owner.0, name),
        Scope::Full => zeo_rt::constants::const_get(owner.0, name),
    }
}

#[derive(Clone, Copy)]
enum Scope {
    /// `rb_const_get_at`: the module itself.
    Own,
    /// `rb_const_get_from`: the module and its ancestors, not `Object`.
    From,
    /// `rb_const_get`: everything, `Object` included.
    Full,
}

fn missing_const(owner: ClassId, name: &str) -> Signal {
    let owner = zeo_rt::dispatch::class_name(owner).unwrap_or("Object".into());
    zeo_rt::builtins::name_error!("uninitialized constant {owner}::{name}")
}

/// # Safety
///
/// `v` must be a live `VALUE`.
unsafe fn const_get_in(v: Value, id: Id, scope: Scope) -> Result<Value, Signal> {
    let owner = unsafe { as_class(v)? };
    let name = symbol_of(id).name_str().to_string();
    let found = const_lookup(owner, &name, scope).ok_or_else(|| missing_const(owner, &name))?;
    to_value(&found)
}

/// MRI's `rb_class_real`: walk past the singleton and `ICLASS` links to the
/// first class an ordinary Ruby program can name. A `T_ICLASS` has no
/// counterpart in zeo, so a singleton is the only thing to unwrap.
fn real_class(cid: ClassId) -> ClassId {
    zeo_rt::runtime_meta::singleton_class_owner(cid)
        .and_then(|owner| zeo_rt::dispatch::class_name(owner).map(|_| owner))
        .map_or(cid, real_class)
}

crate::cext_fn! {
    // ---- objects -------------------------------------------------------

    /// `rb_obj_alloc(klass)`: allocate WITHOUT running `initialize`. That
    /// split is the whole reason it exists -- `rb_class_new_instance` is
    /// this followed by `rb_obj_call_init`.
    fn rb_obj_alloc(klass: Value) -> Value {
        let k = unsafe { value_of(klass) };
        to_value(&send(&k, "allocate", &[])?)
    }

    /// `rb_obj_call_init(obj, argc, argv)`: run `initialize` on an object
    /// `rb_obj_alloc` already made. `initialize` is private, so this is the
    /// implicit-receiver call rather than the explicit one.
    fn rb_obj_call_init(obj: Value, argc: c_int, argv: *const Value) -> () {
        let recv = unsafe { value_of(obj) };
        let args = unsafe { args_of(argc, argv) };
        zeo_rt::dispatch::send_value(&recv, Symbol::intern("initialize"), &args, None)?;
        Ok(())
    }

    /// `rb_obj_call_init_kw`. zeo peels a trailing keyword Hash from the
    /// argument list itself, so the `kw_splat` flag adds nothing here.
    fn rb_obj_call_init_kw(obj: Value, argc: c_int, argv: *const Value, _kw: c_int) -> () {
        unsafe { rb_obj_call_init(obj, argc, argv);
        Ok(()) }
    }

    /// `rb_obj_as_string(v)`: `to_s`, and a non-String answer is stringified
    /// by `rb_any_to_s`'s rule rather than raising.
    fn rb_obj_as_string(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        let out = send(&recv, "to_s", &[])?;
        if matches!(out, RubyValue::Str(_)) {
            return to_value(&out);
        }
        to_value(&send(&recv, "inspect", &[])?)
    }

    fn rb_obj_id(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        to_value(&send(&recv, "object_id", &[])?)
    }

    /// `rb_obj_init_copy(dst, src)`: the `initialize_copy` half of `dup`.
    fn rb_obj_init_copy(dst: Value, src: Value) -> Value {
        let d = unsafe { value_of(dst) };
        let s = unsafe { value_of(src) };
        zeo_rt::dispatch::send_value(&d, Symbol::intern("initialize_copy"), &[s], None)?;
        Ok(dst)
    }

    fn rb_obj_is_instance_of(v: Value, klass: Value) -> Value {
        let recv = unsafe { value_of(v) };
        let k = unsafe { value_of(klass) };
        to_value(&send(&recv, "instance_of?", &[k])?)
    }

    fn rb_obj_is_proc(v: Value) -> Value {
        Ok(super::convert::boolean(matches!(unsafe { value_of(v) }, RubyValue::Proc(_))))
    }

    fn rb_obj_is_method(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        let is = zeo_rt::dispatch::class_name(recv.class_id()).is_some_and(|n| n == "Method");
        Ok(super::convert::boolean(is))
    }

    fn rb_obj_is_fiber(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        let is = zeo_rt::dispatch::class_name(recv.class_id()).is_some_and(|n| n == "Fiber");
        Ok(super::convert::boolean(is))
    }

    /// `rb_obj_respond_to(obj, id, priv)`. The `priv` flag is MRI's "count
    /// private methods too", which is the difference between `respond_to?`'s
    /// one-argument and two-argument forms.
    fn rb_obj_respond_to(v: Value, id: Id, include_private: c_int) -> c_int {
        let recv = unsafe { value_of(v) };
        let name = RubyValue::Symbol(symbol_of(id));
        let args = [name, RubyValue::Bool(include_private != 0)];
        let out = send(&recv, "respond_to?", &args)?;
        Ok(c_int::from(!matches!(out, RubyValue::Nil | RubyValue::Bool(false))))
    }

    fn rb_obj_method_arity(v: Value, id: Id) -> c_int {
        let recv = unsafe { value_of(v) };
        let m = send(&recv, "method", &[RubyValue::Symbol(symbol_of(id))])?;
        match send(&m, "arity", &[])? {
            RubyValue::Int(n) => Ok(n as c_int),
            other => Err(wrong_arg_type(&other, "Integer")),
        }
    }

    fn rb_obj_singleton_methods(argc: c_int, argv: *const Value, recv: Value) -> Value {
        unsafe { send_argv(recv, "singleton_methods", argc, argv) }
    }

    fn rb_obj_instance_eval(argc: c_int, argv: *const Value, recv: Value) -> Value {
        unsafe { send_argv(recv, "instance_eval", argc, argv) }
    }

    fn rb_obj_instance_exec(argc: c_int, argv: *const Value, recv: Value) -> Value {
        unsafe { send_argv(recv, "instance_exec", argc, argv) }
    }

    fn rb_obj_freeze_inline(v: Value) -> () {
        let recv = unsafe { value_of(v) };
        send(&recv, "freeze", &[])?;
        Ok(())
    }

    /// `rb_equal(a, b)`: `a == b`, with an identity short circuit MRI also
    /// takes -- and which matters here, because two handles for one object
    /// are the same pointer.
    fn rb_equal(a: Value, b: Value) -> Value {
        if a == b {
            return Ok(value::Q_TRUE);
        }
        let av = unsafe { value_of(a) };
        let bv = unsafe { value_of(b) };
        to_value(&send(&av, "==", &[bv])?)
    }

    fn rb_eql(a: Value, b: Value) -> c_int {
        let av = unsafe { value_of(a) };
        let bv = unsafe { value_of(b) };
        Ok(c_int::from(super::convert::truthy(to_value(&send(&av, "eql?", &[bv])?)?)))
    }

    fn rb_inspect(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        to_value(&send(&recv, "inspect", &[])?)
    }

    fn rb_any_to_s(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        to_value(&send(&recv, "to_s", &[])?)
    }

    // ---- classes and modules -------------------------------------------

    fn rb_module_new() -> Value {
        to_value(&zeo_rt::runtime_meta::runtime_module_new(None)?)
    }

    /// `rb_class_new(super)`: a new anonymous SUBCLASS of its argument. Not
    /// `Class#new`, which allocates an instance -- see `DENY` in the
    /// forwarding tool for why this one cannot be forwarded.
    fn rb_class_new(superclass: Value) -> Value {
        let sup = unsafe { value_of(superclass) };
        to_value(&zeo_rt::runtime_meta::runtime_class_new(Some(sup), None)?)
    }

    fn rb_class_new_instance(argc: c_int, argv: *const Value, klass: Value) -> Value {
        unsafe { send_argv(klass, "new", argc, argv) }
    }

    fn rb_class_new_instance_kw(argc: c_int, argv: *const Value, klass: Value, _kw: c_int) -> Value {
        unsafe { send_argv(klass, "new", argc, argv) }
    }

    fn rb_class_new_instance_pass_kw(argc: c_int, argv: *const Value, klass: Value) -> Value {
        unsafe { send_argv(klass, "new", argc, argv) }
    }

    /// `rb_class_get_superclass`. A Module has none, and MRI answers `Qfalse`
    /// there rather than `Qnil` -- the two are distinguishable in C and an
    /// extension that tests `== Qnil` would be misled by the wrong one.
    fn rb_class_get_superclass(klass: Value) -> Value {
        let k = unsafe { value_of(klass) };
        match send(&k, "superclass", &[]) {
            Ok(v) => to_value(&v),
            Err(_) => Ok(value::Q_FALSE),
        }
    }

    /// `rb_class_real(klass)`: the first class a Ruby program can name,
    /// walking past a singleton.
    fn rb_class_real(klass: Value) -> Value {
        let cid = unsafe { as_class(klass)? };
        to_value(&RubyValue::Class(real_class(cid)))
    }

    /// `rb_class_name(klass)`: MRI answers the `#<Class:0x..>` form for an
    /// anonymous class where `Class#name` answers nil, so this is `to_s`.
    fn rb_class_name(klass: Value) -> Value {
        let k = unsafe { value_of(klass) };
        to_value(&send(&k, "to_s", &[])?)
    }

    /// `rb_class_path`: the fully qualified name. Same answer as
    /// `rb_class_name` in zeo, because zeo's class names are already
    /// qualified at the point they are recorded.
    fn rb_class_path(klass: Value) -> Value {
        let k = unsafe { value_of(klass) };
        to_value(&send(&k, "to_s", &[])?)
    }

    /// `rb_class_path_cached`: the name only if it is already known, and
    /// `Qnil` rather than a computed one. Anonymous answers nil.
    fn rb_class_path_cached(klass: Value) -> Value {
        let cid = unsafe { as_class(klass)? };
        to_value(&match zeo_rt::dispatch::class_name(cid) {
            Some(n) => zeo_rt::builtins::string::str_value_in_enc(zeo_rt::encoding::UTF_8, &n),
            None => RubyValue::Nil,
        })
    }

    /// `rb_path2class("Foo::Bar")`: resolve a qualified name from `Object`.
    fn rb_path2class(path: *const c_char) -> Value {
        to_value(&resolve_path(&unsafe { cstr(path) })?)
    }

    fn rb_path_to_class(path: Value) -> Value {
        let p = unsafe { value_of(path) };
        let name = match &p {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => return Err(wrong_arg_type(other, "String")),
        };
        to_value(&resolve_path(&name)?)
    }

    fn rb_class_instance_methods(argc: c_int, argv: *const Value, klass: Value) -> Value {
        unsafe { send_argv(klass, "instance_methods", argc, argv) }
    }

    fn rb_class_public_instance_methods(argc: c_int, argv: *const Value, klass: Value) -> Value {
        unsafe { send_argv(klass, "public_instance_methods", argc, argv) }
    }

    fn rb_class_protected_instance_methods(argc: c_int, argv: *const Value, klass: Value) -> Value {
        unsafe { send_argv(klass, "protected_instance_methods", argc, argv) }
    }

    fn rb_class_private_instance_methods(argc: c_int, argv: *const Value, klass: Value) -> Value {
        unsafe { send_argv(klass, "private_instance_methods", argc, argv) }
    }

    fn rb_mod_constants(argc: c_int, argv: *const Value, m: Value) -> Value {
        unsafe { send_argv(m, "constants", argc, argv) }
    }

    fn rb_mod_class_variables(argc: c_int, argv: *const Value, m: Value) -> Value {
        unsafe { send_argv(m, "class_variables", argc, argv) }
    }

    fn rb_mod_module_eval(argc: c_int, argv: *const Value, m: Value) -> Value {
        unsafe { send_argv(m, "module_eval", argc, argv) }
    }

    fn rb_mod_module_exec(argc: c_int, argv: *const Value, m: Value) -> Value {
        unsafe { send_argv(m, "module_exec", argc, argv) }
    }

    fn rb_mod_remove_cvar(m: Value, name: Value) -> Value {
        let mv = unsafe { value_of(m) };
        let n = unsafe { value_of(name) };
        to_value(&send(&mv, "remove_class_variable", &[n])?)
    }

    fn rb_mod_init_copy(dst: Value, src: Value) -> Value {
        let d = unsafe { value_of(dst) };
        let s = unsafe { value_of(src) };
        zeo_rt::dispatch::send_value(&d, Symbol::intern("initialize_copy"), &[s], None)?;
        Ok(dst)
    }

    fn rb_mod_method_arity(m: Value, id: Id) -> c_int {
        let mv = unsafe { value_of(m) };
        let um = send(&mv, "instance_method", &[RubyValue::Symbol(symbol_of(id))])?;
        match send(&um, "arity", &[])? {
            RubyValue::Int(n) => Ok(n as c_int),
            other => Err(wrong_arg_type(&other, "Integer")),
        }
    }

    fn rb_singleton_class(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        to_value(&send(&recv, "singleton_class", &[])?)
    }

    /// `rb_singleton_class_attached(klass, obj)`: MRI records which object a
    /// singleton class belongs to. zeo's singleton classes carry that link
    /// from the moment they are minted, so there is nothing to attach.
    fn rb_singleton_class_attached(_klass: Value, _obj: Value) -> () {
        Ok(())
    }

    // ---- constants -----------------------------------------------------

    fn rb_const_get(m: Value, id: Id) -> Value {
        unsafe { const_get_in(m, id, Scope::Full) }
    }

    fn rb_const_get_at(m: Value, id: Id) -> Value {
        unsafe { const_get_in(m, id, Scope::Own) }
    }

    fn rb_const_get_from(m: Value, id: Id) -> Value {
        unsafe { const_get_in(m, id, Scope::From) }
    }

    fn rb_const_defined(m: Value, id: Id) -> c_int {
        let owner = unsafe { as_class(m)? };
        let name = symbol_of(id).name_str().to_string();
        Ok(c_int::from(const_lookup(owner, &name, Scope::Full).is_some()))
    }

    fn rb_const_defined_at(m: Value, id: Id) -> c_int {
        let owner = unsafe { as_class(m)? };
        let name = symbol_of(id).name_str().to_string();
        Ok(c_int::from(const_lookup(owner, &name, Scope::Own).is_some()))
    }

    fn rb_const_defined_from(m: Value, id: Id) -> c_int {
        let owner = unsafe { as_class(m)? };
        let name = symbol_of(id).name_str().to_string();
        Ok(c_int::from(const_lookup(owner, &name, Scope::From).is_some()))
    }

    fn rb_const_set(m: Value, id: Id, val: Value) -> () {
        let owner = unsafe { as_class(m)? };
        let name = symbol_of(id).name_str().to_string();
        zeo_rt::constants::const_set(owner.0, &name, unsafe { value_of(val) });
        Ok(())
    }

    fn rb_const_remove(m: Value, id: Id) -> Value {
        let owner = unsafe { as_class(m)? };
        let name = symbol_of(id).name_str().to_string();
        let gone = zeo_rt::constants::const_remove(owner.0, &name)
            .ok_or_else(|| missing_const(owner, &name))?;
        to_value(&gone)
    }

    fn rb_define_global_const(name: *const c_char, val: Value) -> () {
        let name = unsafe { cstr(name) };
        zeo_rt::constants::const_set(zeo_abi::OBJECT_CLASS.0, &name, unsafe { value_of(val) });
        Ok(())
    }

    // ---- instance and class variables ----------------------------------

    /// `rb_iv_get(obj, "@x")`. The `@` is part of the name an extension
    /// writes, and MRI keeps it -- so a caller passing `"x"` gets a
    /// `NameError` here as it would there.
    fn rb_iv_get(obj: Value, name: *const c_char) -> Value {
        let recv = unsafe { value_of(obj) };
        let n = ivar_name(&unsafe { cstr(name) });
        to_value(&zeo_rt::dispatch::instance_variable_get(&recv, &n)?)
    }

    fn rb_iv_set(obj: Value, name: *const c_char, val: Value) -> Value {
        let recv = unsafe { value_of(obj) };
        let n = ivar_name(&unsafe { cstr(name) });
        zeo_rt::dispatch::instance_variable_set(&recv, &n, unsafe { value_of(val) })?;
        Ok(val)
    }

    fn rb_ivar_defined(obj: Value, id: Id) -> Value {
        let recv = unsafe { value_of(obj) };
        let bare = symbol_of(id).name_str().trim_start_matches('@').to_string();
        Ok(super::convert::boolean(zeo_rt::dispatch::ivar_defined(&recv, &bare)))
    }

    fn rb_ivar_count(obj: Value) -> usize {
        let recv = unsafe { value_of(obj) };
        match send(&recv, "instance_variables", &[])? {
            RubyValue::Array(a) => Ok(a.lock().len()),
            _ => Ok(0),
        }
    }

    /// `rb_ivar_foreach(obj, f, arg)`. `f` answers `ST_CONTINUE` (0) to keep
    /// going; anything else stops, which is what `rb_hash_foreach` does too.
    fn rb_ivar_foreach(
        obj: Value,
        f: unsafe extern "C" fn(Id, Value, usize) -> c_int,
        arg: usize,
    ) -> () {
        let recv = unsafe { value_of(obj) };
        let names = match send(&recv, "instance_variables", &[])? {
            RubyValue::Array(a) => a.lock().to_vec(),
            _ => Vec::new(),
        };
        for name in names {
            let val = zeo_rt::dispatch::instance_variable_get(&recv, &name)?;
            let RubyValue::Symbol(s) = &name else { continue };
            let (id, val) = ((*s).to_u32() as Id, to_value(&val)?);
            let go = super::jmp::protect(|| unsafe { f(id, val, arg) })?;
            if go != 0 {
                break;
            }
        }
        Ok(())
    }

    fn rb_cvar_get(m: Value, id: Id) -> Value {
        let mv = unsafe { value_of(m) };
        to_value(&send(&mv, "class_variable_get", &[RubyValue::Symbol(symbol_of(id))])?)
    }

    fn rb_cvar_set(m: Value, id: Id, val: Value) -> () {
        let mv = unsafe { value_of(m) };
        let v = unsafe { value_of(val) };
        send(&mv, "class_variable_set", &[RubyValue::Symbol(symbol_of(id)), v])?;
        Ok(())
    }

    fn rb_cvar_defined(m: Value, id: Id) -> Value {
        let mv = unsafe { value_of(m) };
        to_value(&send(&mv, "class_variable_defined?", &[RubyValue::Symbol(symbol_of(id))])?)
    }

    /// `rb_cvar_find(m, id, *where)`: the value, and the module it was found
    /// on. zeo's lookup does not report the owner, so `*where` answers the
    /// module that was asked -- which is what MRI answers whenever the
    /// variable is not inherited.
    fn rb_cvar_find(m: Value, id: Id, found_in: *mut Value) -> Value {
        let out = unsafe { rb_cvar_get(m, id) };
        if !found_in.is_null() {
            // SAFETY: the caller's own `VALUE` slot.
            unsafe { found_in.write(m) };
        }
        Ok(out)
    }

    fn rb_define_class_variable(m: Value, name: *const c_char, val: Value) -> () {
        let mv = unsafe { value_of(m) };
        let n = Symbol::intern(&unsafe { cstr(name) });
        let v = unsafe { value_of(val) };
        send(&mv, "class_variable_set", &[RubyValue::Symbol(n), v])?;
        Ok(())
    }

    // ---- checks and coercions ------------------------------------------

    /// `rb_check_frozen(obj)`: raise `FrozenError` if it is frozen.
    fn rb_check_frozen(v: Value) -> () {
        let recv = unsafe { value_of(v) };
        if super::convert::truthy(to_value(&send(&recv, "frozen?", &[])?)?) {
            let what = send(&recv, "inspect", &[])
                .ok()
                .and_then(|s| match s {
                    RubyValue::Str(s) => Some(s.lock().to_utf8_lossy().into_owned()),
                    _ => None,
                })
                .unwrap_or_else(|| "object".into());
            return Err(zeo_rt::builtins::frozen_error!("can't modify frozen {}: {what}",
                    zeo_rt::dispatch::class_name(recv.class_id()).unwrap_or("Object".into())));
        }
        Ok(())
    }

    /// `rb_check_copyable(obj, orig)`: `dup`/`clone`'s own guard. The
    /// destination must not be frozen, and the two must be the same class.
    fn rb_check_copyable(obj: Value, orig: Value) -> () {
        unsafe { rb_check_frozen(obj) };
        let a = unsafe { value_of(obj) };
        let b = unsafe { value_of(orig) };
        if a.class_id() != b.class_id() {
            return Err(zeo_rt::builtins::type_error!("initialize_copy should take same class object"));
        }
        Ok(())
    }

    /// `rb_check_inheritable(klass)`: a superclass has to be a Class, and
    /// must not be a singleton class.
    fn rb_check_inheritable(klass: Value) -> () {
        let v = unsafe { value_of(klass) };
        let RubyValue::Class(cid) = v else {
            return Err(wrong_arg_type(&v, "Class"));
        };
        if zeo_rt::runtime_meta::singleton_class_owner(cid).is_some() {
            return Err(zeo_rt::builtins::type_error!("can't make subclass of singleton class"));
        }
        Ok(())
    }

    /// `rb_check_safe_str`: the `$SAFE` check, which Ruby removed in 3.0.
    /// The type check is what survives of it.
    fn rb_check_safe_str(v: Value) -> () {
        let s = unsafe { value_of(v) };
        if !matches!(s, RubyValue::Str(_)) {
            return Err(wrong_arg_type(&s, "String"));
        }
        Ok(())
    }

    /// `rb_check_convert_type(v, type, tname, method)`: call `method`, and
    /// answer nil rather than raising if it is absent. The `type` code is
    /// checked against the ANSWER, so a `to_ary` returning a String is a
    /// `TypeError` here as it is in MRI.
    fn rb_check_convert_type(
        v: Value,
        want: c_int,
        tname: *const c_char,
        method: *const c_char,
    ) -> Value {
        let recv = unsafe { value_of(v) };
        let meth = unsafe { cstr(method) };
        let tname = unsafe { cstr(tname) };
        let Some(out) = try_convert(&recv, &meth) else {
            return Ok(value::Q_NIL);
        };
        if matches!(out, RubyValue::Nil) {
            return Ok(value::Q_NIL);
        }
        let got = unsafe { super::handles::type_tag(to_value(&out)?) } as c_int;
        if want != 0 && got != want {
            return Err(conversion_error(&recv, &meth, &tname));
        }
        to_value(&out)
    }

    /// `rb_check_to_integer(v, method)`: the same, fixed to Integer.
    fn rb_check_to_integer(v: Value, method: *const c_char) -> Value {
        let recv = unsafe { value_of(v) };
        let meth = unsafe { cstr(method) };
        match try_convert(&recv, &meth) {
            Some(out @ (RubyValue::Int(_) | RubyValue::BigInt(_))) => to_value(&out),
            _ => Ok(value::Q_NIL),
        }
    }

    fn rb_check_to_int(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        match try_convert(&recv, "to_int") {
            Some(out @ (RubyValue::Int(_) | RubyValue::BigInt(_))) => to_value(&out),
            _ => Ok(value::Q_NIL),
        }
    }

    fn rb_check_to_float(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        match try_convert(&recv, "to_f") {
            Some(out @ RubyValue::Float(_)) => to_value(&out),
            _ => Ok(value::Q_NIL),
        }
    }

    /// `rb_check_type(v, type)`: raise unless the `RUBY_T_*` tag matches.
    fn rb_check_type(v: Value, want: c_int) -> () {
        let recv = unsafe { value_of(v) };
        let got = unsafe { super::handles::type_tag(v) } as c_int;
        if got != want {
            return Err(wrong_arg_type(&recv, &format!("T_{want}")));
        }
        Ok(())
    }

    /// `rb_to_int(v)`: `to_int`, and a `TypeError` if it is absent. The
    /// difference from `rb_check_to_int` is exactly that raise.
    fn rb_to_int(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        to_value(&must_convert(&recv, "to_int", "Integer")?)
    }

    fn rb_to_float(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        to_value(&must_convert(&recv, "to_f", "Float")?)
    }

    fn rb_to_symbol(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        to_value(&must_convert(&recv, "to_sym", "Symbol")?)
    }

    fn rb_to_id(v: Value) -> Id {
        let recv = unsafe { value_of(v) };
        match must_convert(&recv, "to_sym", "Symbol")? {
            RubyValue::Symbol(s) => Ok(s.to_u32() as Id),
            other => Err(wrong_arg_type(&other, "Symbol")),
        }
    }

    /// `rb_check_id(&v)`: the `ID` for a Symbol or String, and `0` when the
    /// name has never been interned. An extension uses the zero to skip a
    /// lookup that cannot succeed, so minting a new ID here would defeat it.
    ///
    /// The argument is `volatile VALUE *` because MRI rewrites it in place
    /// with the converted Symbol; this does the same.
    fn rb_check_id(slot: *mut Value) -> Id {
        if slot.is_null() {
            return Ok(0);
        }
        // SAFETY: the caller's own `VALUE` slot.
        let v = unsafe { value_of(slot.read()) };
        let name = match &v {
            RubyValue::Symbol(s) => s.name_str().to_string(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => return Err(wrong_arg_type(other, "String or Symbol")),
        };
        match Symbol::interned(&name) {
            Some(sym) => {
                unsafe { slot.write(to_value(&RubyValue::Symbol(sym))?) };
                Ok(sym.to_u32() as Id)
            }
            None => Ok(0),
        }
    }

    /// `rb_check_symbol(&v)`: the same question, answered as a Symbol or
    /// `Qnil` rather than an `ID` or `0`.
    fn rb_check_symbol(slot: *mut Value) -> Value {
        let id = unsafe { rb_check_id(slot) };
        if id == 0 {
            return Ok(value::Q_NIL);
        }
        to_value(&RubyValue::Symbol(symbol_of(id)))
    }

    /// `rb_check_id_cstr(p, len, enc)`: the same question asked of C bytes
    /// rather than a `VALUE`, so there is no slot to rewrite and no
    /// conversion to make. `0` still means "never interned", and still means
    /// the caller can skip a lookup that cannot succeed.
    ///
    /// The `enc` is what MRI tags the name with while looking it up. zeo's
    /// symbol table is keyed by the name's characters rather than by its
    /// bytes plus an encoding, so two names that differ only in encoding are
    /// one symbol here -- which is the same rule `rb_intern3` already
    /// follows, and the reason it ignores its own `enc` too.
    fn rb_check_id_cstr(p: *const c_char, len: c_long, _enc: *const std::ffi::c_void) -> Id {
        let bytes = unsafe { super::string::borrow_bytes(p, len) };
        let name = String::from_utf8_lossy(&bytes).into_owned();
        Ok(Symbol::interned(&name).map_or(0, |s| s.to_u32() as Id))
    }

    fn rb_check_symbol_cstr(p: *const c_char, len: c_long, enc: *const std::ffi::c_void) -> Value {
        let id = unsafe { rb_check_id_cstr(p, len, enc) };
        if id == 0 {
            return Ok(value::Q_NIL);
        }
        to_value(&RubyValue::Symbol(symbol_of(id)))
    }

    // ---- ID predicates -------------------------------------------------

    fn rb_is_const_id(id: Id) -> c_int {
        Ok(c_int::from(symbol_of(id).name_str().starts_with(char::is_uppercase)))
    }

    fn rb_is_class_id(id: Id) -> c_int {
        Ok(c_int::from(symbol_of(id).name_str().starts_with("@@")))
    }

    fn rb_is_instance_id(id: Id) -> c_int {
        let s = symbol_of(id);
        let s = s.name_str();
        Ok(c_int::from(s.starts_with('@') && !s.starts_with("@@")))
    }

    fn rb_is_global_id(id: Id) -> c_int {
        Ok(c_int::from(symbol_of(id).name_str().starts_with('$')))
    }

    fn rb_is_attrset_id(id: Id) -> c_int {
        let s = symbol_of(id);
        let s = s.name_str();
        Ok(c_int::from(s.ends_with('=') && s.len() > 1 && !s.ends_with("==")))
    }

    /// `rb_is_local_id`: a plain lowercase name -- not a constant, not a
    /// variable sigil, not an assignment, and not an operator.
    fn rb_is_local_id(id: Id) -> c_int {
        let s = symbol_of(id);
        let s = s.name_str();
        let plain = s.starts_with(|c: char| c.is_lowercase() || c == '_')
            && s.chars().all(|c| c.is_alphanumeric() || c == '_');
        Ok(c_int::from(plain))
    }

    /// `rb_is_junk_id`: anything none of the others claim.
    fn rb_is_junk_id(id: Id) -> c_int {
        let any = unsafe {
            rb_is_const_id(id) | rb_is_class_id(id) | rb_is_instance_id(id)
                | rb_is_global_id(id) | rb_is_attrset_id(id) | rb_is_local_id(id)
        };
        Ok(c_int::from(any == 0))
    }

    /// `rb_is_absolute_path(p)`. A POSIX path is absolute when it starts with
    /// a separator; `~` is MRI's own extra case, because it expands to one.
    fn rb_is_absolute_path(p: *const c_char) -> c_int {
        let s = unsafe { cstr(p) };
        Ok(c_int::from(s.starts_with('/') || s.starts_with('~')))
    }

    // ---- global variables ----------------------------------------------

    /// `rb_gv_get(name)`. A name `rb_define_variable` bound to a C word is
    /// read from that word, because C may have written it directly since the
    /// last time anything published it -- see [`super::builtins`].
    fn rb_gv_get(name: *const c_char) -> Value {
        let n = gvar_name(&unsafe { cstr(name) });
        if let Some(v) = super::builtins::slot_get(&n) {
            return Ok(v);
        }
        to_value(&zeo_rt::globals::global_get(MAIN_BOX, &n))
    }

    fn rb_gv_set(name: *const c_char, val: Value) -> Value {
        let n = gvar_name(&unsafe { cstr(name) });
        if super::builtins::slot_set(&n, val)? {
            return Ok(val);
        }
        zeo_rt::globals::global_set(MAIN_BOX, &n, unsafe { value_of(val) });
        Ok(val)
    }

    /// `rb_gvar_readonly_setter`: MRI's setter for a variable it refuses to
    /// let anyone write. It is `noreturn` there and raises here.
    fn rb_gvar_readonly_setter(_val: Value, id: Id, _data: *mut Value) -> () {
        Err(zeo_rt::builtins::name_error!("{} is a read-only variable", symbol_of(id).name_str()))
    }
}

/// `"x"` and `"@x"` both mean `@x` to `rb_iv_get`. MRI's own entry takes the
/// name with the sigil, and every use in the census writes it -- but the
/// bare form costs one comparison to accept and would otherwise be a
/// `NameError` naming a variable the extension believes it set.
fn ivar_name(name: &str) -> RubyValue {
    let full = if name.starts_with('@') {
        name.to_string()
    } else {
        format!("@{name}")
    };
    RubyValue::Symbol(Symbol::intern(&full))
}

/// `rb_gv_get("x")` and `rb_gv_get("$x")` name one variable, the same way.
fn gvar_name(name: &str) -> String {
    if name.starts_with('$') {
        name.to_string()
    } else {
        format!("${name}")
    }
}

/// `Foo::Bar::Baz` from `Object`. Every segment must be a class or module,
/// which is what makes this a `path2CLASS`.
fn resolve_path(path: &str) -> Result<RubyValue, Signal> {
    let mut owner = zeo_abi::OBJECT_CLASS;
    let mut found = RubyValue::Class(owner);
    for seg in path.split("::").filter(|s| !s.is_empty()) {
        let v =
            zeo_rt::constants::const_get(owner.0, seg).ok_or_else(|| missing_const(owner, seg))?;
        match &v {
            RubyValue::Class(cid) => owner = *cid,
            other => {
                return Err(zeo_rt::builtins::type_error!(
                    "{path} does not refer to class/module ({} is a {})",
                    seg,
                    zeo_rt::dispatch::class_name(other.class_id()).unwrap_or("value".into())
                ));
            }
        }
        found = v;
    }
    Ok(found)
}

/// Call a conversion method, or answer `None` when the receiver has none.
/// A raise from INSIDE the method still travels: only the absence is caught,
/// which is the distinction MRI's `rb_check_funcall` draws too.
fn try_convert(recv: &RubyValue, meth: &str) -> Option<RubyValue> {
    let sym = Symbol::intern(meth);
    if !zeo_rt::dispatch::responds_to_value(recv, sym, true) {
        return None;
    }
    zeo_rt::dispatch::send_value(recv, sym, &[], None).ok()
}

fn conversion_error(recv: &RubyValue, meth: &str, tname: &str) -> Signal {
    zeo_rt::builtins::type_error!(
        "can't convert {} to {tname} ({}#{meth} gives the wrong type)",
        zeo_rt::dispatch::class_name(recv.class_id()).unwrap_or("Object".into()),
        zeo_rt::dispatch::class_name(recv.class_id()).unwrap_or("Object".into())
    )
}

fn must_convert(recv: &RubyValue, meth: &str, tname: &str) -> Result<RubyValue, Signal> {
    try_convert(recv, meth).ok_or_else(|| {
        zeo_rt::builtins::type_error!(
            "no implicit conversion of {} into {tname}",
            zeo_rt::dispatch::class_name(recv.class_id()).unwrap_or("Object".into())
        )
    })
}
