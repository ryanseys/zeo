//! The rest: the `rb_define_*` remainder, Set, Time, Regexp, Marshal, Range
//! and the Symbol table.
//!
//! # `rb_define_variable` hands out a `VALUE *`
//!
//! `rb_define_variable("$foo", &my_slot)` makes a Ruby global whose storage
//! is the extension's own C word. Reading `$foo` reads that word; writing it
//! writes it. There is no Ruby object in between, and MRI's GC scans the
//! slot to keep whatever it holds alive.
//!
//! zeo cannot register a slot with its global table, which stores
//! `RubyValue`s -- so the four entries that do this ([`rb_define_variable`]
//! and its hooked, virtual and readonly siblings) install a Ruby-side
//! accessor pair that reads and writes the C word through a raw pointer, and
//! the slot is pinned for the life of the process so its contents cannot be
//! collected.
//!
//! # `rb_range_beg_len` is a protocol, not a getter
//!
//! It decides three different things -- "not a Range", "out of bounds" and
//! "here are the clamped bounds" -- through two out-parameters and a
//! tri-state return. Getting the encoding wrong turns an out-of-range slice
//! into a silently clamped one.

use super::convert::{to_value, value_of};
use super::object::{args_of, cstr, send};
use super::symbol::{Id, symbol_of};
use super::value::{self, Value};
use std::ffi::{c_char, c_int, c_long};
use zeo_rt::builtins::wrong_arg_type;
use zeo_rt::{RubyValue, Signal, Symbol};

fn class_named(name: &str) -> Result<RubyValue, Signal> {
    zeo_rt::constants::const_get(zeo_abi::OBJECT_CLASS.0, name)
        .ok_or_else(|| zeo_rt::builtins::name_error!("uninitialized constant {name}"))
}

fn a_string(text: &str) -> RubyValue {
    zeo_rt::builtins::string::str_value_in_enc(zeo_rt::encoding::UTF_8, text)
}

/// # Safety
///
/// `v` must be a live `VALUE`.
unsafe fn as_class(v: Value) -> Result<zeo_rt::dispatch::ClassId, Signal> {
    unsafe { super::object::as_class(v) }
}

/// The visibility `rb_define_attr`'s two flags ask for, applied by defining
/// the reader, the writer or both.
fn define_attr(owner: Value, name: &str, read: bool, write: bool) -> Result<(), Signal> {
    let k = unsafe { value_of(owner) };
    let sym = RubyValue::Symbol(Symbol::intern(name));
    if read {
        send(&k, "attr_reader", std::slice::from_ref(&sym))?;
    }
    if write {
        send(&k, "attr_writer", &[sym])?;
    }
    Ok(())
}

crate::cext_fn! {
    // ---- the define remainder --------------------------------------------

    fn rb_define_class_id(id: Id, superclass: Value) -> Value {
        let sup = unsafe { value_of(superclass) };
        to_value(&zeo_rt::runtime_meta::runtime_class_new(Some(sup), None)?)
            .and_then(|v| name_and_home(v, zeo_abi::OBJECT_CLASS, symbol_of(id).name_str()))
    }

    fn rb_define_class_id_under(outer: Value, id: Id, superclass: Value) -> Value {
        let home = unsafe { as_class(outer)? };
        let sup = unsafe { value_of(superclass) };
        to_value(&zeo_rt::runtime_meta::runtime_class_new(Some(sup), None)?)
            .and_then(|v| name_and_home(v, home, symbol_of(id).name_str()))
    }

    fn rb_define_module_id(id: Id) -> Value {
        to_value(&zeo_rt::runtime_meta::runtime_module_new(None)?)
            .and_then(|v| name_and_home(v, zeo_abi::OBJECT_CLASS, symbol_of(id).name_str()))
    }

    fn rb_define_module_id_under(outer: Value, id: Id) -> Value {
        let home = unsafe { as_class(outer)? };
        to_value(&zeo_rt::runtime_meta::runtime_module_new(None)?)
            .and_then(|v| name_and_home(v, home, symbol_of(id).name_str()))
    }

    fn rb_define_method_id(klass: Value, id: Id, f: super::method::MethodPtr, argc: c_int) -> () {
        let owner = unsafe { as_class(klass)? };
        // SAFETY: the caller promised the shape `argc` names.
        let body = unsafe { super::method::method_proc(f, argc)? };
        zeo_rt::runtime_meta::runtime_define_method(owner, symbol_of(id), body)?;
        Ok(())
    }

    /// `rb_define_module_function(mod, name, f, argc)`: both a private
    /// instance method AND a singleton method, which is what
    /// `module_function` means.
    fn rb_define_module_function(
        module: Value,
        name: *const c_char,
        f: super::method::MethodPtr,
        argc: c_int,
    ) -> () {
        let owner = unsafe { as_class(module)? };
        let sym = Symbol::intern(&unsafe { cstr(name) });
        // SAFETY: the caller promised the shape.
        let body = unsafe { super::method::method_proc(f, argc)? };
        zeo_rt::runtime_meta::runtime_define_method(owner, sym, body)?;
        zeo_rt::runtime_meta::runtime_module_function(owner, &[RubyValue::Symbol(sym)])?;
        Ok(())
    }

    /// `rb_define_global_function(name, f, argc)`: a private method on
    /// `Object`, which is what makes it callable with no receiver anywhere.
    fn rb_define_global_function(
        name: *const c_char,
        f: super::method::MethodPtr,
        argc: c_int,
    ) -> () {
        let sym = Symbol::intern(&unsafe { cstr(name) });
        // SAFETY: the caller promised the shape.
        let body = unsafe { super::method::method_proc(f, argc)? };
        zeo_rt::runtime_meta::runtime_define_method(zeo_abi::OBJECT_CLASS, sym, body)?;
        zeo_rt::runtime_meta::runtime_set_visibility(
            zeo_abi::OBJECT_CLASS,
            &[RubyValue::Symbol(sym)],
            zeo_rt::dispatch::MethodVisibility::Private,
        )?;
        Ok(())
    }

    fn rb_define_attr(klass: Value, name: *const c_char, read: c_int, write: c_int) -> () {
        define_attr(klass, &unsafe { cstr(name) }, read != 0, write != 0)
    }

    /// `rb_attr(klass, id, read, write, ex)`: the same, with `ex` asking for
    /// the CALLER's current visibility rather than public. A C extension has
    /// no such visibility, so public is the only honest answer -- and it is
    /// what MRI answers when the caller is not inside a Ruby frame.
    fn rb_attr(klass: Value, id: Id, read: c_int, write: c_int, _ex: c_int) -> () {
        define_attr(klass, symbol_of(id).name_str(), read != 0, write != 0)
    }

    fn rb_alias(klass: Value, new: Id, old: Id) -> () {
        let owner = unsafe { as_class(klass)? };
        zeo_rt::runtime_meta::runtime_alias_method(owner, symbol_of(new), symbol_of(old))?;
        Ok(())
    }

    fn rb_undef(klass: Value, id: Id) -> () {
        let owner = unsafe { as_class(klass)? };
        zeo_rt::runtime_meta::runtime_undef_method(owner, &[RubyValue::Symbol(symbol_of(id))])?;
        Ok(())
    }

    fn rb_undef_method(klass: Value, name: *const c_char) -> () {
        let owner = unsafe { as_class(klass)? };
        let sym = Symbol::intern(&unsafe { cstr(name) });
        zeo_rt::runtime_meta::runtime_undef_method(owner, &[RubyValue::Symbol(sym)])?;
        Ok(())
    }

    fn rb_remove_method(klass: Value, name: *const c_char) -> () {
        let owner = unsafe { as_class(klass)? };
        let sym = Symbol::intern(&unsafe { cstr(name) });
        zeo_rt::runtime_meta::runtime_remove_method(owner, &[RubyValue::Symbol(sym)])?;
        Ok(())
    }

    fn rb_remove_method_id(klass: Value, id: Id) -> () {
        let owner = unsafe { as_class(klass)? };
        zeo_rt::runtime_meta::runtime_remove_method(owner, &[RubyValue::Symbol(symbol_of(id))])?;
        Ok(())
    }

    fn rb_prepend_module(target: Value, module: Value) -> () {
        let t = unsafe { value_of(target) };
        let m = unsafe { value_of(module) };
        zeo_rt::runtime_meta::runtime_prepend(&t, &[m])?;
        Ok(())
    }

    fn rb_deprecate_constant(klass: Value, name: *const c_char) -> () {
        let k = unsafe { value_of(klass) };
        let n = a_string(&unsafe { cstr(name) });
        send(&k, "deprecate_constant", &[n])?;
        Ok(())
    }

    fn rb_cv_get(klass: Value, name: *const c_char) -> Value {
        let k = unsafe { value_of(klass) };
        let n = RubyValue::Symbol(Symbol::intern(&cvar_name(&unsafe { cstr(name) })));
        to_value(&send(&k, "class_variable_get", &[n])?)
    }

    fn rb_cv_set(klass: Value, name: *const c_char, v: Value) -> () {
        let k = unsafe { value_of(klass) };
        let n = RubyValue::Symbol(Symbol::intern(&cvar_name(&unsafe { cstr(name) })));
        let val = unsafe { value_of(v) };
        send(&k, "class_variable_set", &[n, val])?;
        Ok(())
    }

    /// `rb_attr_get(obj, id)`: read an ivar WITHOUT the "not initialized"
    /// warning `rb_ivar_get` can emit. Both answer nil for a missing one.
    fn rb_attr_get(obj: Value, id: Id) -> Value {
        let recv = unsafe { value_of(obj) };
        let name = RubyValue::Symbol(symbol_of(id));
        to_value(&zeo_rt::dispatch::instance_variable_get(&recv, &name).unwrap_or(RubyValue::Nil))
    }

    /// `rb_set_class_path(klass, under, name)`: give an anonymous class a
    /// name and a home, which is what `Foo::Bar = Class.new` does.
    fn rb_set_class_path(klass: Value, under: Value, name: *const c_char) -> () {
        let cid = unsafe { as_class(klass)? };
        let home = if under == value::Q_NIL || under == 0 {
            zeo_abi::OBJECT_CLASS
        } else {
            unsafe { as_class(under)? }
        };
        install_name(cid, home, &unsafe { cstr(name) });
        Ok(())
    }

    fn rb_set_class_path_string(klass: Value, under: Value, name: Value) -> () {
        let text = match unsafe { value_of(name) } {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => return Err(wrong_arg_type(&other, "String")),
        };
        let cid = unsafe { as_class(klass)? };
        let home = if under == value::Q_NIL || under == 0 {
            zeo_abi::OBJECT_CLASS
        } else {
            unsafe { as_class(under)? }
        };
        install_name(cid, home, &text);
        Ok(())
    }

    /// `rb_freeze_singleton_class(v)`: freezing an object freezes its
    /// singleton class too, so a frozen object cannot gain a method.
    fn rb_freeze_singleton_class(v: Value) -> () {
        let recv = unsafe { value_of(v) };
        if let Ok(sc) = send(&recv, "singleton_class", &[]) {
            sc.freeze_value()?;
        }
        Ok(())
    }

    fn rb_refinement_new() -> Value {
        to_value(&zeo_rt::runtime_meta::runtime_module_new(None)?)
    }

    fn rb_binding_new() -> Value {
        to_value(&zeo_rt::dispatch::send_value(
            &zeo_rt::dispatch::main_object(),
            Symbol::intern("binding"),
            &[],
            None,
        )?)
    }

    // ---- global variables backed by a C slot -----------------------------

    /// `rb_define_variable(name, &slot)`: a Ruby global whose STORAGE is the
    /// extension's own C word. See this module's docs.
    fn rb_define_variable(name: *const c_char, slot: *mut Value) -> () {
        bind_slot(&unsafe { cstr(name) }, slot, true)
    }

    /// `rb_define_readonly_variable`: the same, and an assignment raises.
    fn rb_define_readonly_variable(name: *const c_char, slot: *const Value) -> () {
        bind_slot(&unsafe { cstr(name) }, slot.cast_mut(), false)
    }

    /// `rb_define_hooked_variable(name, &slot, get, set)`: the slot plus a
    /// pair of C callbacks. A null callback means "use the slot", which is
    /// how MRI spells a half-hooked variable.
    fn rb_define_hooked_variable(
        name: *const c_char,
        slot: *mut Value,
        get: Option<unsafe extern "C-unwind" fn(Id, *mut Value) -> Value>,
        set: Option<unsafe extern "C-unwind" fn(Value, Id, *mut Value)>,
    ) -> () {
        bind_hooked(&unsafe { cstr(name) }, slot, get, set)
    }

    /// `rb_define_virtual_variable(name, get, set)`: callbacks and no slot
    /// at all.
    fn rb_define_virtual_variable(
        name: *const c_char,
        get: Option<unsafe extern "C-unwind" fn(Id, *mut Value) -> Value>,
        set: Option<unsafe extern "C-unwind" fn(Value, Id, *mut Value)>,
    ) -> () {
        bind_hooked(&unsafe { cstr(name) }, std::ptr::null_mut(), get, set)
    }

    fn rb_alias_variable(new: Id, old: Id) -> () {
        zeo_rt::globals::global_alias(0, symbol_of(new).name_str(), symbol_of(old).name_str());
        Ok(())
    }

    fn rb_f_trace_var(argc: c_int, argv: *const Value) -> Value {
        kernel("trace_var", argc, argv)
    }

    fn rb_f_untrace_var(argc: c_int, argv: *const Value) -> Value {
        kernel("untrace_var", argc, argv)
    }

    // ---- Set -------------------------------------------------------------

    fn rb_set_new() -> Value {
        to_value(&send(&class_named("Set")?, "new", &[])?)
    }

    fn rb_set_new_capa(_capa: usize) -> Value {
        unsafe { Ok(rb_set_new()) }
    }

    /// `rb_set_add`: answers whether the element was NEW, which is what
    /// `Set#add?` reports through nil.
    fn rb_set_add(set: Value, item: Value) -> bool {
        let s = unsafe { value_of(set) };
        let v = unsafe { value_of(item) };
        Ok(!matches!(send(&s, "add?", &[v])?, RubyValue::Nil))
    }

    fn rb_set_delete(set: Value, item: Value) -> bool {
        let s = unsafe { value_of(set) };
        let v = unsafe { value_of(item) };
        let had = super::convert::truthy(to_value(&send(&s, "include?", std::slice::from_ref(&v))?)?);
        send(&s, "delete", &[v])?;
        Ok(had)
    }

    fn rb_set_lookup(set: Value, item: Value) -> bool {
        let s = unsafe { value_of(set) };
        let v = unsafe { value_of(item) };
        Ok(super::convert::truthy(to_value(&send(&s, "include?", &[v])?)?))
    }

    fn rb_set_size(set: Value) -> usize {
        let s = unsafe { value_of(set) };
        match send(&s, "size", &[])? {
            RubyValue::Int(n) => Ok(n.max(0) as usize),
            _ => Ok(0),
        }
    }

    /// `rb_set_foreach(set, f, arg)`: `f` answers `ST_CONTINUE` (0) to keep
    /// going, as the `st_table` walks do.
    fn rb_set_foreach(
        set: Value,
        f: unsafe extern "C-unwind" fn(Value, Value) -> c_int,
        arg: Value,
    ) -> () {
        let s = unsafe { value_of(set) };
        let items = match send(&s, "to_a", &[])? {
            RubyValue::Array(a) => a.lock().to_vec(),
            _ => Vec::new(),
        };
        for item in items {
            let v = to_value(&item)?;
            if super::unwind::protect(|| unsafe { f(v, arg) })? != 0 {
                break;
            }
        }
        Ok(())
    }

    // ---- Time ------------------------------------------------------------

    fn rb_time_new(sec: i64, usec: c_long) -> Value {
        time_at(sec, usec * 1_000)
    }

    fn rb_time_nano_new(sec: i64, nsec: c_long) -> Value {
        time_at(sec, nsec)
    }

    fn rb_time_num_new(seconds: Value, offset: Value) -> Value {
        let s = unsafe { value_of(seconds) };
        let off = unsafe { value_of(offset) };
        let time = class_named("Time")?;
        let built = send(&time, "at", &[s])?;
        if matches!(off, RubyValue::Nil) {
            return to_value(&built);
        }
        to_value(&send(&built, "localtime", &[off])?)
    }

    fn rb_time_timespec_new(ts: *const Timespec, _offset: c_int) -> Value {
        if ts.is_null() {
            return time_at(0, 0);
        }
        // SAFETY: the caller's own `struct timespec`.
        let t = unsafe { ts.read() };
        time_at(t.tv_sec, t.tv_nsec)
    }

    /// `rb_time_timeval(v)` / `rb_time_timespec(v)`: the Time as a C struct.
    /// A `struct` return crosses the ABI by value, which is why these are
    /// `#[repr(C)]` types rather than out-parameters.
    fn rb_time_timeval(v: Value) -> Timeval {
        let (sec, nsec) = time_parts(v)?;
        Ok(Timeval {
            tv_sec: sec,
            tv_usec: (nsec / 1_000) as i32,
        })
    }

    fn rb_time_timespec(v: Value) -> Timespec {
        let (sec, nsec) = time_parts(v)?;
        Ok(Timespec {
            tv_sec: sec,
            tv_nsec: nsec,
        })
    }

    /// `rb_time_interval(v)`: a DURATION rather than a moment, so a Numeric
    /// is seconds and a negative one is an `ArgumentError`.
    fn rb_time_interval(v: Value) -> Timeval {
        let (sec, nsec) = interval_parts(v)?;
        Ok(Timeval {
            tv_sec: sec,
            tv_usec: (nsec / 1_000) as i32,
        })
    }

    fn rb_time_timespec_interval(v: Value) -> Timespec {
        let (sec, nsec) = interval_parts(v)?;
        Ok(Timespec {
            tv_sec: sec,
            tv_nsec: nsec,
        })
    }

    fn rb_timespec_now(out: *mut Timespec) -> () {
        if out.is_null() {
            return Ok(());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        // SAFETY: the caller's own `struct timespec`.
        unsafe {
            out.write(Timespec {
                tv_sec: now.as_secs() as i64,
                tv_nsec: i64::from(now.subsec_nanos()),
            });
        }
        Ok(())
    }

    // ---- Regexp ----------------------------------------------------------

    fn rb_reg_new(p: *const c_char, len: c_long, options: c_int) -> Value {
        let bytes = unsafe { super::string::borrow_bytes(p, len) };
        let src = zeo_rt::builtins::string::str_value_in_enc(
            zeo_rt::encoding::UTF_8,
            &String::from_utf8_lossy(&bytes),
        );
        new_regexp(src, options)
    }

    fn rb_reg_new_str(src: Value, options: c_int) -> Value {
        new_regexp(unsafe { value_of(src) }, options)
    }

    /// `rb_reg_init_str(re, src, options)`: initialize an ALREADY allocated
    /// Regexp. zeo's Regexp is immutable once built, so this answers a new
    /// one -- and every caller in the census uses the answer rather than the
    /// object it passed in.
    fn rb_reg_init_str(_re: Value, src: Value, options: c_int) -> Value {
        new_regexp(unsafe { value_of(src) }, options)
    }

    /// `rb_reg_alloc()`: an uninitialized Regexp, for `rb_reg_init_str` to
    /// fill. zeo has no such half-built state; an empty pattern is the
    /// closest object that behaves, and `rb_reg_init_str` replaces it.
    fn rb_reg_alloc() -> Value {
        new_regexp(a_string(""), 0)
    }

    fn rb_reg_options(re: Value) -> c_int {
        let r = unsafe { value_of(re) };
        match send(&r, "options", &[])? {
            RubyValue::Int(n) => Ok(n as c_int),
            _ => Ok(0),
        }
    }

    /// `rb_reg_quote(str)`: `Regexp.escape`, so text can be spliced into a
    /// pattern as a literal.
    fn rb_reg_quote(s: Value) -> Value {
        let text = unsafe { value_of(s) };
        to_value(&send(&class_named("Regexp")?, "escape", &[text])?)
    }

    /// `rb_reg_regcomp(str)`: `Regexp.new(str)` with no options. MRI caches
    /// the compiled result against the source string; zeo compiles each time,
    /// which costs a little and cannot be observed -- the answer is a fresh
    /// equal Regexp either way.
    fn rb_reg_regcomp(s: Value) -> Value {
        new_regexp(unsafe { value_of(s) }, 0)
    }

    /// `rb_char_to_option_kcode(c, &option, &kcode)`: read one `/pattern/x`
    /// flag letter.
    ///
    /// `option` takes the `Regexp::IGNORECASE`/`EXTENDED`/`MULTILINE` bit, or
    /// `FIXEDENCODING`/`NOENCODING` for a letter that names an encoding
    /// instead. `kcode` takes that encoding's index, and `-1` when the letter
    /// named none. The numbers are ruby's own public `Regexp` constants,
    /// which is what makes them checkable rather than remembered.
    ///
    /// An unknown letter answers 0 with `*option` 0 -- MRI's own way of
    /// saying "not a flag", and the caller's cue to reject it.
    fn rb_char_to_option_kcode(c: c_int, option: *mut c_int, kcode: *mut c_int) -> c_int {
        const IGNORECASE: c_int = 1;
        const EXTENDED: c_int = 2;
        const MULTILINE: c_int = 4;
        const FIXEDENCODING: c_int = 16;
        const NOENCODING: c_int = 32;

        let named = |name: &str| c_int::from(zeo_rt::encoding::find(name).map_or(0, |e| e.0));
        let (opt, code) = match u8::try_from(c).unwrap_or(0) {
            b'n' => (NOENCODING, named("ASCII-8BIT")),
            b'e' => (FIXEDENCODING, named("EUC-JP")),
            b's' => (FIXEDENCODING, named("Windows-31J")),
            b'u' => (FIXEDENCODING, named("UTF-8")),
            b'i' => (IGNORECASE, -1),
            b'x' => (EXTENDED, -1),
            b'm' => (MULTILINE, -1),
            _ => (0, -1),
        };
        if !option.is_null() {
            // SAFETY: the caller's own `int`.
            unsafe { option.write(opt) };
        }
        if !kcode.is_null() {
            // SAFETY: as above.
            unsafe { kcode.write(code) };
        }
        // The encoding letters answer 1 rather than their bit, which is what
        // MRI does: the caller counts them, it does not or them together.
        Ok(match opt {
            0 => 0,
            FIXEDENCODING => 1,
            other => other,
        })
    }

    /// `rb_reg_match(re, str)`: the match POSITION, as `=~` answers -- NOT a
    /// MatchData, which is why it cannot be a forwarded row.
    fn rb_reg_match(re: Value, s: Value) -> Value {
        let r = unsafe { value_of(re) };
        let text = unsafe { value_of(s) };
        to_value(&send(&r, "=~", &[text])?)
    }

    /// `rb_reg_match2(re)`: match against `$_`, the last line read.
    fn rb_reg_match2(re: Value) -> Value {
        let r = unsafe { value_of(re) };
        let line = zeo_rt::globals::global_get(0, "$_");
        to_value(&send(&r, "=~", &[line])?)
    }

    fn rb_reg_last_match(m: Value) -> Value {
        match_part(m, "to_s")
    }

    fn rb_reg_match_pre(m: Value) -> Value {
        match_part(m, "pre_match")
    }

    fn rb_reg_match_post(m: Value) -> Value {
        match_part(m, "post_match")
    }

    fn rb_reg_match_last(m: Value) -> Value {
        let md = unsafe { value_of(m) };
        if matches!(md, RubyValue::Nil) {
            return Ok(value::Q_NIL);
        }
        // The last group that PARTICIPATED, which is not always the last
        // group -- an alternation leaves the others nil.
        let n = match send(&md, "size", &[])? {
            RubyValue::Int(n) => n,
            _ => 0,
        };
        for i in (1..n).rev() {
            let g = send(&md, "[]", &[RubyValue::Int(i)])?;
            if !matches!(g, RubyValue::Nil) {
                return to_value(&g);
            }
        }
        Ok(value::Q_NIL)
    }

    fn rb_reg_nth_match(n: c_int, m: Value) -> Value {
        let md = unsafe { value_of(m) };
        if matches!(md, RubyValue::Nil) {
            return Ok(value::Q_NIL);
        }
        to_value(&send(&md, "[]", &[RubyValue::Int(i64::from(n))])?)
    }

    fn rb_reg_nth_defined(n: c_int, m: Value) -> Value {
        let got = unsafe { rb_reg_nth_match(n, m) };
        Ok(super::convert::boolean(got != value::Q_NIL))
    }

    /// `rb_reg_backref_number(match, name)`: which group a named capture is.
    fn rb_reg_backref_number(m: Value, name: Value) -> c_int {
        let md = unsafe { value_of(m) };
        let n = unsafe { value_of(name) };
        let re = send(&md, "regexp", &[])?;
        let names = match send(&re, "names", &[])? {
            RubyValue::Array(a) => a.lock().to_vec(),
            _ => Vec::new(),
        };
        let want = n.to_display_string();
        match names.iter().position(|v| v.to_display_string() == want) {
            Some(i) => Ok(i as c_int + 1),
            None => Err(zeo_rt::builtins::index_error!("undefined group name reference: {want}")),
        }
    }

    /// `rb_match_busy(m)`: MRI marks a MatchData as in use so a later match
    /// on the same thread cannot recycle its buffer. zeo's MatchData owns
    /// its own, so there is nothing to reserve.
    fn rb_match_busy(_m: Value) -> () {
        Ok(())
    }

    // ---- Marshal ---------------------------------------------------------

    /// `rb_marshal_dump(obj, port)`: a nil port answers the String.
    fn rb_marshal_dump(obj: Value, port: Value) -> Value {
        let o = unsafe { value_of(obj) };
        let p = unsafe { value_of(port) };
        let args = if matches!(p, RubyValue::Nil) {
            vec![o]
        } else {
            vec![o, p]
        };
        to_value(&send(&class_named("Marshal")?, "dump", &args)?)
    }

    fn rb_marshal_load(port: Value) -> Value {
        let p = unsafe { value_of(port) };
        to_value(&send(&class_named("Marshal")?, "load", &[p])?)
    }

    // ---- Range -----------------------------------------------------------

    /// `rb_range_new(beg, end, exclude_end)`.
    fn rb_range_new(beg: Value, end: Value, excl: c_int) -> Value {
        let cls = class_named("Range")?;
        let args = [
            unsafe { value_of(beg) },
            unsafe { value_of(end) },
            RubyValue::Bool(excl != 0),
        ];
        to_value(&send(&cls, "new", &args)?)
    }

    /// `rb_range_values(range, &beg, &end, &excl)`: answers 1 for a Range,
    /// 0 for anything else. An extension uses the 0 to fall through to its
    /// non-Range path rather than raising.
    fn rb_range_values(v: Value, beg: *mut Value, end: *mut Value, excl: *mut c_int) -> c_int {
        let r = unsafe { value_of(v) };
        if zeo_rt::dispatch::class_name(r.class_id()).as_deref() != Some("Range") {
            return Ok(0);
        }
        let b = send(&r, "begin", &[])?;
        let e = send(&r, "end", &[])?;
        let x = send(&r, "exclude_end?", &[])?;
        if !beg.is_null() {
            // SAFETY: the caller's own `VALUE` slot.
            unsafe { beg.write(to_value(&b)?) };
        }
        if !end.is_null() {
            unsafe { end.write(to_value(&e)?) };
        }
        if !excl.is_null() {
            // SAFETY: the caller's own `int`.
            unsafe { excl.write(c_int::from(matches!(x, RubyValue::Bool(true)))) };
        }
        Ok(1)
    }

    /// `rb_range_beg_len(range, &beg, &len, alen, err)`: clamp a Range
    /// against a collection of length `alen`. Three answers, and each means
    /// something different to the caller:
    ///
    /// | Answer | Means |
    /// |---|---|
    /// | `Qfalse` | not a Range at all -- try the other argument shapes |
    /// | `Qnil` | a Range, but out of bounds |
    /// | `Qtrue` | `*beg` and `*len` are set |
    ///
    /// `err == 0` softens the out-of-bounds case; nonzero raises.
    fn rb_range_beg_len(
        range: Value,
        beg: *mut c_long,
        len: *mut c_long,
        alen: c_long,
        err: c_int,
    ) -> Value {
        let r = unsafe { value_of(range) };
        if zeo_rt::dispatch::class_name(r.class_id()).as_deref() != Some("Range") {
            return Ok(value::Q_FALSE);
        }
        let excl = matches!(send(&r, "exclude_end?", &[])?, RubyValue::Bool(true));
        let b = endpoint(&send(&r, "begin", &[])?, 0)?;
        let e = endpoint(&send(&r, "end", &[])?, alen)?;
        let mut start = if b < 0 { b + alen } else { b };
        if start < 0 || start > alen {
            if err != 0 {
                return Err(zeo_rt::builtins::range_error!("{} out of range", r.to_display_string()));
            }
            return Ok(value::Q_NIL);
        }
        let mut stop = if e < 0 { e + alen } else { e };
        if !excl {
            stop += 1;
        }
        stop = stop.clamp(start, alen);
        if start > alen {
            start = alen;
        }
        if !beg.is_null() {
            // SAFETY: the caller's own `long`.
            unsafe { beg.write(start) };
        }
        if !len.is_null() {
            unsafe { len.write(stop - start) };
        }
        Ok(value::Q_TRUE)
    }

    // ---- symbols ---------------------------------------------------------

    fn rb_id2str(id: Id) -> Value {
        to_value(&a_string(symbol_of(id).name_str()))
    }

    /// `rb_id_attrset(id)`: the `name=` form of a name. MRI mints it, so a
    /// caller can define the setter for an attribute it only has the reader
    /// name for.
    fn rb_id_attrset(id: Id) -> Id {
        let name = symbol_of(id).name_str();
        Ok(Symbol::intern(&format!("{name}=")).to_u32() as Id)
    }

    fn rb_sym_all_symbols() -> Value {
        to_value(&send(&class_named("Symbol")?, "all_symbols", &[])?)
    }

    /// `rb_symname_p(name)`: could this text be a Symbol written literally?
    /// A name that needs quoting -- `:"a b"` -- answers false.
    fn rb_symname_p(name: *const c_char) -> c_int {
        let s = unsafe { cstr(name) };
        Ok(c_int::from(plain_symbol_name(&s)))
    }
}

/// `struct timespec` and `struct timeval`, which four entries return BY
/// VALUE. Their layout is the platform's and is read by C.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Timeval {
    pub tv_sec: i64,
    pub tv_usec: i32,
}

/// Name a fresh class and put it under `home`, which is what makes it
/// findable and what `#inspect` prints.
fn name_and_home(v: Value, home: zeo_rt::dispatch::ClassId, name: &str) -> Result<Value, Signal> {
    if let RubyValue::Class(cid) = unsafe { value_of(v) } {
        install_name(cid, home, name);
    }
    Ok(v)
}

fn install_name(cid: zeo_rt::dispatch::ClassId, home: zeo_rt::dispatch::ClassId, name: &str) {
    let full = match zeo_rt::dispatch::class_name(home) {
        Some(o) if home != zeo_abi::OBJECT_CLASS => format!("{o}::{name}"),
        _ => name.to_string(),
    };
    zeo_rt::runtime_meta::name_runtime_class_if_anonymous(cid, &full);
    zeo_rt::constants::const_set(home.0, name, RubyValue::Class(cid));
}

/// `"x"` and `"@@x"` both name `@@x`, the way `rb_iv_get` accepts either
/// spelling of an instance variable.
fn cvar_name(name: &str) -> String {
    if name.starts_with("@@") {
        name.to_string()
    } else {
        format!("@@{}", name.trim_start_matches('@'))
    }
}

fn kernel(meth: &str, argc: c_int, argv: *const Value) -> Result<Value, Signal> {
    let args = unsafe { args_of(argc, argv) };
    to_value(&zeo_rt::dispatch::send_value(
        &zeo_rt::dispatch::main_object(),
        Symbol::intern(meth),
        &args,
        None,
    )?)
}

/// Every C-backed global, by name. The slot address and the two hooks; a
/// null slot is a purely virtual variable.
static SLOTS: std::sync::Mutex<Option<std::collections::HashMap<String, Slot>>> =
    std::sync::Mutex::new(None);

struct Slot {
    /// The extension's own `VALUE` word, as an address so the row is `Send`.
    /// It lives in the loaded image for the life of the process.
    addr: usize,
    writable: bool,
    get: Option<unsafe extern "C-unwind" fn(Id, *mut Value) -> Value>,
    set: Option<unsafe extern "C-unwind" fn(Value, Id, *mut Value)>,
}

// SAFETY: `addr` is an address in the loaded extension's own image, and the
// two function pointers are its own functions. Neither moves, and access is
// serialised by the `Mutex`.
unsafe impl Send for Slot {}

fn with_slots<R>(f: impl FnOnce(&mut std::collections::HashMap<String, Slot>) -> R) -> Option<R> {
    let mut guard = SLOTS.lock().ok()?;
    Some(f(guard.get_or_insert_with(std::collections::HashMap::new)))
}

fn bind_slot(name: &str, slot: *mut Value, writable: bool) -> Result<(), Signal> {
    register_slot(name, slot, writable, None, None)
}

fn bind_hooked(
    name: &str,
    slot: *mut Value,
    get: Option<unsafe extern "C-unwind" fn(Id, *mut Value) -> Value>,
    set: Option<unsafe extern "C-unwind" fn(Value, Id, *mut Value)>,
) -> Result<(), Signal> {
    register_slot(name, slot, set.is_some() || !slot.is_null(), get, set)
}

/// Publish the slot's CURRENT value into zeo's global table and remember the
/// address, so a later read through Ruby sees what C wrote.
///
/// This is the honest limit of the design: zeo's global table stores values,
/// not addresses, so a write C makes directly to its own word is visible only
/// after something calls back through here. `rb_gv_get` on such a name
/// re-reads the slot, which is the path every Ruby-level read takes.
fn register_slot(
    name: &str,
    slot: *mut Value,
    writable: bool,
    get: Option<unsafe extern "C-unwind" fn(Id, *mut Value) -> Value>,
    set: Option<unsafe extern "C-unwind" fn(Value, Id, *mut Value)>,
) -> Result<(), Signal> {
    let full = if name.starts_with('$') {
        name.to_string()
    } else {
        format!("${name}")
    };
    with_slots(|s| {
        s.insert(
            full.clone(),
            Slot {
                addr: slot as usize,
                writable,
                get,
                set,
            },
        );
    });
    // Seed the table so a read before any C write answers what the slot
    // already holds, rather than nil.
    if !slot.is_null() {
        // SAFETY: the extension's own word, live for the process.
        let held = unsafe { slot.read() };
        if !value::is_special_const(held) {
            super::handles::pin_forever(held);
        }
        zeo_rt::globals::global_set(0, &full, unsafe { value_of(held) });
    }
    Ok(())
}

/// Read a C-backed global, if `name` is one. [`super::object`]'s `rb_gv_get`
/// asks here first.
pub(super) fn slot_get(name: &str) -> Option<Value> {
    with_slots(|s| {
        let row = s.get(name)?;
        let id = Symbol::intern(name).to_u32() as Id;
        if let Some(get) = row.get {
            // SAFETY: the extension's own getter, on its own slot.
            return Some(unsafe { get(id, row.addr as *mut Value) });
        }
        if row.addr == 0 {
            return None;
        }
        // SAFETY: the extension's own word.
        Some(unsafe { (row.addr as *const Value).read() })
    })
    .flatten()
}

/// Write one. Answers false when `name` is not a C-backed global, and raises
/// when it is a read-only one.
pub(super) fn slot_set(name: &str, v: Value) -> Result<bool, Signal> {
    let found = with_slots(|s| {
        let row = s.get(name)?;
        Some((row.addr, row.writable, row.set))
    })
    .flatten();
    let Some((addr, writable, set)) = found else {
        return Ok(false);
    };
    if !writable {
        return Err(zeo_rt::builtins::name_error!(
            "{name} is a read-only variable"
        ));
    }
    let id = Symbol::intern(name).to_u32() as Id;
    if let Some(set) = set {
        // SAFETY: the extension's own setter, on its own slot.
        unsafe { set(v, id, addr as *mut Value) };
        return Ok(true);
    }
    if addr == 0 {
        return Ok(false);
    }
    if !value::is_special_const(v) {
        // The word is not a place zeo can trace, so what it holds is pinned
        // for the process. MRI's own GC scans the slot instead.
        super::handles::pin_forever(v);
    }
    // SAFETY: the extension's own word.
    unsafe { (addr as *mut Value).write(v) };
    Ok(true)
}

fn time_at(sec: i64, nsec: i64) -> Result<Value, Signal> {
    let time = class_named("Time")?;
    let args = [RubyValue::Int(sec), RubyValue::Int(nsec / 1_000)];
    to_value(&send(&time, "at", &args)?)
}

/// A Time's whole and fractional parts.
fn time_parts(v: Value) -> Result<(i64, i64), Signal> {
    let t = unsafe { value_of(v) };
    let sec = match send(&t, "to_i", &[])? {
        RubyValue::Int(n) => n,
        other => return Err(wrong_arg_type(&other, "Integer")),
    };
    let nsec = match send(&t, "nsec", &[]) {
        Ok(RubyValue::Int(n)) => n,
        _ => 0,
    };
    Ok((sec, nsec))
}

/// A DURATION's parts. A Numeric is seconds; a negative one is an
/// `ArgumentError`, which is what `sleep(-1)` raises.
fn interval_parts(v: Value) -> Result<(i64, i64), Signal> {
    let t = unsafe { value_of(v) };
    let secs = match &t {
        RubyValue::Int(n) => *n as f64,
        RubyValue::Float(f) => *f,
        other => match send(other, "to_f", &[])? {
            RubyValue::Float(f) => f,
            _ => return Err(wrong_arg_type(other, "Numeric")),
        },
    };
    if secs < 0.0 {
        return Err(zeo_rt::builtins::arg_error!(
            "time interval must not be negative"
        ));
    }
    Ok((secs.trunc() as i64, (secs.fract() * 1e9) as i64))
}

fn new_regexp(src: RubyValue, options: c_int) -> Result<Value, Signal> {
    let cls = class_named("Regexp")?;
    to_value(&send(
        &cls,
        "new",
        &[src, RubyValue::Int(i64::from(options))],
    )?)
}

/// One part of a MatchData, with `nil` passing straight through -- every
/// `rb_reg_match_*` entry answers nil for a nil match rather than raising.
fn match_part(m: Value, meth: &str) -> Result<Value, Signal> {
    let md = unsafe { value_of(m) };
    if matches!(md, RubyValue::Nil) {
        return Ok(value::Q_NIL);
    }
    to_value(&send(&md, meth, &[])?)
}

/// A Range endpoint, with `nil` meaning the collection's own edge.
fn endpoint(v: &RubyValue, default: c_long) -> Result<c_long, Signal> {
    match v {
        RubyValue::Nil => Ok(default),
        RubyValue::Int(n) => Ok(*n as c_long),
        other => match send(other, "to_int", &[])? {
            RubyValue::Int(n) => Ok(n as c_long),
            _ => Err(wrong_arg_type(other, "Integer")),
        },
    }
}

/// Could `s` be written as a bare `:symbol`? Constants, locals, ivars,
/// globals, setters and the operator names all can; anything else needs
/// quoting.
///
/// `rb_enc_symname_p` asks the same question with an encoding, which changes
/// nothing: every spelling that parses bare is ASCII.
pub(super) fn symname_is_plain(s: &str) -> bool {
    plain_symbol_name(s)
}

fn plain_symbol_name(s: &str) -> bool {
    const OPERATORS: &[&str] = &[
        "+", "-", "*", "/", "%", "**", "==", "!=", "<", "<=", ">", ">=", "<=>", "===", "=~", "!~",
        "<<", ">>", "&", "|", "^", "~", "!", "[]", "[]=", "+@", "-@", "`",
    ];
    if s.is_empty() {
        return false;
    }
    if OPERATORS.contains(&s) {
        return true;
    }
    let body = s
        .strip_prefix("@@")
        .or_else(|| s.strip_prefix('@'))
        .or_else(|| s.strip_prefix('$'))
        .unwrap_or(s);
    let body = body.strip_suffix('=').unwrap_or(body);
    let body = body.strip_suffix(['?', '!']).unwrap_or(body);
    !body.is_empty()
        && body.starts_with(|c: char| c.is_alphabetic() || c == '_')
        && body.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `rb_symname_p` decides whether a name needs quoting, and both
    /// directions matter: a false positive prints `:a b`, which does not
    /// parse.
    #[test]
    fn a_plain_symbol_name_is_one_that_parses_bare() {
        for ok in [
            "foo", "Foo", "_x", "a1", "foo?", "foo!", "foo=", "@iv", "@@cv", "$g", "+", "[]",
            "[]=", "<=>", "===",
        ] {
            assert!(plain_symbol_name(ok), "{ok} should need no quoting");
        }
        for bad in ["", "a b", "1abc", "a-b", "a.b", "@", "@@", "$"] {
            assert!(!plain_symbol_name(bad), "{bad:?} should need quoting");
        }
    }

    /// `"x"`, `"@x"` and `"@@x"` all name the class variable `@@x`, the way
    /// `rb_iv_get` accepts a bare instance variable name.
    #[test]
    fn a_class_variable_name_gains_its_sigil() {
        assert_eq!(cvar_name("x"), "@@x");
        assert_eq!(cvar_name("@x"), "@@x");
        assert_eq!(cvar_name("@@x"), "@@x");
    }
}
