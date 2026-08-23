//! The long tail: conversions, keyword extraction, globs, event hooks and
//! the entries that only raise.
//!
//! Nothing here belongs with a family, and most of it is one function long.
//! What they have in common is that each is reachable from a real gem and
//! each has a definite right answer.
//!
//! # `rb_get_kwargs` is the C spelling of a keyword parameter list
//!
//! It takes a Hash, a table of `ID`s, how many are required and how many are
//! optional, and fills a `VALUE` array. The subtleties are all in the
//! counting: a negative `optional` means "and collect the rest into a Hash",
//! an absent optional is `Qundef` rather than `Qnil` -- so the callee can
//! tell "not given" from "given as nil" -- and a leftover key is an
//! `ArgumentError` unless the rest-Hash was asked for.

use super::convert::{to_value, value_of};
use super::object::{args_of, cstr, send, wrong_type};
use super::symbol::{Id, symbol_of};
use super::value::{self, Value};
use crate::{RubyValue, Signal, Symbol};
use std::ffi::{c_char, c_int, c_long, c_void};

fn a_string(text: &str) -> RubyValue {
    crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, text)
}

fn class_named(name: &str) -> Result<RubyValue, Signal> {
    crate::constants::const_get(zeo_abi::OBJECT_CLASS.0, name).ok_or_else(|| {
        crate::dispatch::raise_error("NameError", format!("uninitialized constant {name}"))
    })
}

crate::cext_fn! {
    // ---- conversions -----------------------------------------------------

    fn rb_Hash(v: Value) -> Value {
        let val = unsafe { value_of(v) };
        to_value(&crate::dispatch::send_value(
            &crate::dispatch::main_object(),
            Symbol::intern("Hash"),
            &[val],
            None,
        )?)
    }

    /// `rb_convert_type(v, type, tname, method)`: `rb_check_convert_type`
    /// that RAISES rather than answering nil. That is the whole difference,
    /// and it is what makes `rb_convert_type` safe to use without a check.
    fn rb_convert_type(
        v: Value,
        want: c_int,
        tname: *const c_char,
        method: *const c_char,
    ) -> Value {
        let recv = unsafe { value_of(v) };
        let meth = unsafe { cstr(method) };
        let tname = unsafe { cstr(tname) };
        let out = send(&recv, &meth, &[]).map_err(|_| no_conversion(&recv, &tname))?;
        let raw = to_value(&out)?;
        // SAFETY: `raw` was just pinned.
        if want != 0 && unsafe { super::handles::type_tag(raw) } as c_int != want {
            return Err(no_conversion(&recv, &tname));
        }
        Ok(raw)
    }

    /// `rb_cmpint(cmp, a, b)`: an `Integer#<=>` answer as a C `int`, and a
    /// nil is the "cannot be compared" raise rather than a zero.
    fn rb_cmpint(cmp: Value, a: Value, b: Value) -> c_int {
        match unsafe { value_of(cmp) } {
            RubyValue::Nil => Err(cmperr(a, b)),
            RubyValue::Int(n) => Ok(n.signum() as c_int),
            other => match send(&other, "<=>", &[RubyValue::Int(0)])? {
                RubyValue::Int(n) => Ok(n.signum() as c_int),
                _ => Err(cmperr(a, b)),
            },
        }
    }

    fn rb_cmperr(a: Value, b: Value) -> () {
        Err(cmperr(a, b))
    }

    /// `rb_memory_id(v)`: the identity a `#object_id` is derived from. For a
    /// heap object that is its handle's address, which is canonical per
    /// object -- so two `VALUE`s for one object answer one id.
    fn rb_memory_id(v: Value) -> Value {
        to_value(&RubyValue::Int(v as i64))
    }

    fn rb_uint2inum(n: usize) -> Value {
        to_value(&crate::builtins::integer::int_value(
            num_bigint::BigInt::from(n as u64),
        ))
    }

    fn rb_uint2big(n: usize) -> Value {
        unsafe { Ok(rb_uint2inum(n)) }
    }

    fn rb_flt_rationalize(v: Value) -> Value {
        let f = unsafe { value_of(v) };
        to_value(&send(&f, "rationalize", &[])?)
    }

    fn rb_flt_rationalize_with_prec(v: Value, prec: Value) -> Value {
        let f = unsafe { value_of(v) };
        let p = unsafe { value_of(prec) };
        to_value(&send(&f, "rationalize", &[p])?)
    }

    /// `rb_external_str_new` / `rb_locale_str_new` / `rb_filesystem_str_new`:
    /// bytes that came from OUTSIDE the process, tagged with the encoding
    /// that boundary uses. All three are UTF-8 on every target zeo builds
    /// extensions for, which is why they share one body.
    fn rb_external_str_new(p: *const c_char, len: c_long) -> Value {
        outside_str(p, len)
    }

    fn rb_external_str_new_cstr(p: *const c_char) -> Value {
        outside_str(p, -1)
    }

    fn rb_locale_str_new(p: *const c_char, len: c_long) -> Value {
        outside_str(p, len)
    }

    fn rb_locale_str_new_cstr(p: *const c_char) -> Value {
        outside_str(p, -1)
    }

    fn rb_filesystem_str_new(p: *const c_char, len: c_long) -> Value {
        outside_str(p, len)
    }

    fn rb_filesystem_str_new_cstr(p: *const c_char) -> Value {
        outside_str(p, -1)
    }

    /// `rb_must_asciicompat(str)`: raise unless the encoding is
    /// ASCII-compatible. An extension calls it before treating the bytes as
    /// text with ASCII delimiters, where UTF-16 would silently mis-split.
    fn rb_must_asciicompat(v: Value) -> () {
        let s = unsafe { value_of(v) };
        let RubyValue::Str(s) = &s else {
            return Err(wrong_type(&s, "String"));
        };
        let enc = s.lock().encoding();
        if !enc.ascii_compatible() {
            return Err(crate::dispatch::raise_error(
                "Encoding::CompatibilityError",
                format!("ASCII incompatible encoding: {}", enc.name()),
            ));
        }
        Ok(())
    }

    /// `rb_uv_to_utf8(buf, uv)`: one code point as UTF-8. Answers the byte
    /// count, and the buffer must hold 6 -- MRI's own contract, and it
    /// accepts the out-of-Unicode range its old 6-byte form allowed.
    fn rb_uv_to_utf8(buf: *mut c_char, uv: usize) -> c_int {
        let Some(ch) = u32::try_from(uv).ok().and_then(char::from_u32) else {
            return Err(crate::dispatch::raise_error(
                "RangeError",
                format!("pack(U): value out of range: {uv}"),
            ));
        };
        let mut tmp = [0u8; 4];
        let bytes = ch.encode_utf8(&mut tmp).as_bytes();
        if !buf.is_null() {
            // SAFETY: the caller promised a 6-byte buffer.
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf.cast::<u8>(), bytes.len()) };
        }
        Ok(bytes.len() as c_int)
    }

    /// `rb_memcicmp(a, b, len)`: `memcmp` folding ASCII case, which is what
    /// a header-name comparison wants.
    fn rb_memcicmp(a: *const c_void, b: *const c_void, len: c_long) -> c_int {
        let n = len.max(0) as usize;
        if a.is_null() || b.is_null() || n == 0 {
            return Ok(0);
        }
        // SAFETY: the caller promised `len` readable bytes in each.
        let (x, y) = unsafe {
            (
                std::slice::from_raw_parts(a.cast::<u8>(), n),
                std::slice::from_raw_parts(b.cast::<u8>(), n),
            )
        };
        for (p, q) in x.iter().zip(y) {
            let (p, q) = (p.to_ascii_lowercase(), q.to_ascii_lowercase());
            if p != q {
                return Ok(c_int::from(p) - c_int::from(q));
            }
        }
        Ok(0)
    }

    /// `rb_mem_clear(ptr, len)`: fill a `VALUE` array with `Qnil`. NOT zero
    /// -- a zeroed slot is `Qfalse`, and an extension that then reads it
    /// gets `false` where it expected `nil`.
    fn rb_mem_clear(p: *mut Value, len: c_long) -> () {
        if p.is_null() {
            return Ok(());
        }
        for i in 0..len.max(0) {
            // SAFETY: the caller promised `len` writable `VALUE`s.
            unsafe { p.offset(i as isize).write(value::Q_NIL) };
        }
        Ok(())
    }

    // ---- keyword arguments -----------------------------------------------

    /// `rb_get_kwargs(hash, table, required, optional, values)`.
    ///
    /// A negative `optional` means "take `-optional - 1` optionals and
    /// collect the rest into a Hash in the last slot". An optional that is
    /// absent is `Qundef`, not `Qnil`, so a callee can tell "not given" from
    /// "given as nil". Answers how many slots were filled.
    fn rb_get_kwargs(
        hash: Value,
        table: *const Id,
        required: c_int,
        optional: c_int,
        values: *mut Value,
    ) -> c_int {
        let h = match unsafe { value_of(hash) } {
            RubyValue::Hash(h) => h,
            RubyValue::Nil => crate::value::collections::hash_new(Vec::new()),
            other => return Err(wrong_type(&other, "Hash")),
        };
        let rest_wanted = optional < 0;
        let opt = if rest_wanted { -optional - 1 } else { optional };
        let named = (required.max(0) + opt.max(0)) as usize;
        let mut taken: Vec<Symbol> = Vec::with_capacity(named);
        for i in 0..named {
            // SAFETY: the caller promised `required + |optional|` IDs.
            let id = unsafe { table.add(i).read() };
            let sym = symbol_of(id);
            taken.push(sym);
            let key = RubyValue::Symbol(sym);
            let found = crate::value::collections::hash_lookup(&h, &key);
            let slot = match (found, i < required.max(0) as usize) {
                (Some(v), _) => to_value(&v)?,
                // A missing REQUIRED key is an error naming it.
                (None, true) => {
                    return Err(crate::dispatch::raise_error(
                        "ArgumentError",
                        format!("missing keyword: :{}", sym.name_str()),
                    ));
                }
                (None, false) => value::Q_UNDEF,
            };
            if !values.is_null() {
                // SAFETY: the caller promised that many writable slots.
                unsafe { values.add(i).write(slot) };
            }
        }
        // What is left over: a rest-Hash if asked for, an error if not.
        let rest: Vec<(RubyValue, RubyValue)> = h
            .lock()
            .iter()
            .filter(|(_, (k, _))| !matches!(k, RubyValue::Symbol(s) if taken.contains(s)))
            .map(|(_, (k, v))| (k.clone(), v.clone()))
            .collect();
        if rest_wanted {
            let leftover = RubyValue::Hash(crate::value::collections::hash_new(rest));
            if !values.is_null() {
                // SAFETY: one more slot, which a negative `optional` asks for.
                unsafe { values.add(named).write(to_value(&leftover)?) };
            }
            return Ok(named as c_int + 1);
        }
        if let Some((k, _)) = rest.first() {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("unknown keyword: {}", k.to_display_string()),
            ));
        }
        Ok(named as c_int)
    }

    /// `rb_extract_keywords(&hash)`: split a trailing keyword Hash off. The
    /// Symbol-keyed pairs come back as the answer and `*hash` keeps the
    /// rest, or becomes `Qnil` when nothing is left.
    fn rb_extract_keywords(slot: *mut Value) -> Value {
        if slot.is_null() {
            return Ok(value::Q_NIL);
        }
        // SAFETY: the caller's own `VALUE` slot.
        let raw = unsafe { slot.read() };
        let RubyValue::Hash(h) = (unsafe { value_of(raw) }) else {
            return Ok(value::Q_NIL);
        };
        let (syms, rest): (Vec<_>, Vec<_>) = h
            .lock()
            .iter()
            .map(|(_, (k, v))| (k.clone(), v.clone()))
            .partition(|(k, _)| matches!(k, RubyValue::Symbol(_)));
        let leftover = if rest.is_empty() {
            value::Q_NIL
        } else {
            to_value(&RubyValue::Hash(crate::value::collections::hash_new(rest)))?
        };
        unsafe { slot.write(leftover) };
        to_value(&RubyValue::Hash(crate::value::collections::hash_new(syms)))
    }

    /// `rb_get_values_at(obj, olen, argc, argv, func)`: `#values_at`'s
    /// worker. Each argument is an index or a Range, and `func` reads one
    /// element.
    fn rb_get_values_at(
        obj: Value,
        olen: c_long,
        argc: c_int,
        argv: *const Value,
        func: unsafe extern "C" fn(Value, c_long) -> Value,
    ) -> Value {
        let mut out: Vec<RubyValue> = Vec::new();
        for arg in unsafe { args_of(argc, argv) } {
            let raw = to_value(&arg)?;
            let mut beg: c_long = 0;
            let mut len: c_long = 0;
            // SAFETY: two `long`s this frame owns.
            let ranged = unsafe {
                super::builtins::rb_range_beg_len(raw, &raw mut beg, &raw mut len, olen, 0)
            };
            if ranged == value::Q_TRUE {
                for i in beg..beg + len {
                    // SAFETY: the caller's own reader, on an in-range index.
                    out.push(unsafe { value_of(func(obj, i)) });
                }
                continue;
            }
            if ranged == value::Q_NIL {
                // A Range that is out of bounds contributes nothing, which
                // is what `values_at` does with one.
                continue;
            }
            let idx = match &arg {
                RubyValue::Int(n) => *n as c_long,
                other => match send(other, "to_int", &[])? {
                    RubyValue::Int(n) => n as c_long,
                    _ => return Err(wrong_type(other, "Integer")),
                },
            };
            // SAFETY: the caller's own reader; it range-checks itself, which
            // is why MRI passes the raw index too.
            out.push(unsafe { value_of(func(obj, idx)) });
        }
        to_value(&RubyValue::Array(crate::value::collections::array_new(out)))
    }

    // ---- objects ---------------------------------------------------------

    /// `rb_obj_hide(v)`: MRI clears the object's class pointer so
    /// `ObjectSpace` and `#inspect` cannot see it -- an internal-object
    /// trick. zeo has no hidden state on a value, and hiding it would break
    /// every later dispatch on the same `VALUE`. Answering it unchanged
    /// leaves the object usable, which is what every caller then does.
    fn rb_obj_hide(v: Value) -> Value {
        Ok(v)
    }

    fn rb_obj_reveal(v: Value, _klass: Value) -> Value {
        Ok(v)
    }

    /// `rb_obj_setup(obj, klass, type)`: MRI writes the class and the
    /// `T_*` tag into an already-allocated object's header. zeo's handle
    /// takes both at construction and neither can move afterwards.
    fn rb_obj_setup(obj: Value, _klass: Value, _type: Value) -> Value {
        Ok(obj)
    }

    fn rb_singleton_class_clone(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        let sc = send(&recv, "singleton_class", &[])?;
        to_value(&send(&sc, "clone", &[])?)
    }

    /// `rb_class_descendants(klass)`: every class that inherits from it.
    /// zeo has no subclass index, so the answer is built by asking each
    /// known class -- which is what `ObjectSpace.each_object(Class)` does.
    fn rb_class_descendants(klass: Value) -> Value {
        let cid = unsafe { super::object::as_class(klass)? };
        let mut out: Vec<RubyValue> = Vec::new();
        for name in crate::dispatch::nested_class_names(zeo_abi::OBJECT_CLASS) {
            if let Some(v @ RubyValue::Class(other)) =
                crate::constants::const_get(zeo_abi::OBJECT_CLASS.0, &name)
                && other != cid
                && crate::dispatch::is_a(other, cid)
            {
                out.push(v);
            }
        }
        to_value(&RubyValue::Array(crate::value::collections::array_new(out)))
    }

    fn rb_define_finalizer(obj: Value, proc: Value) -> Value {
        objspace_call("define_finalizer", &[obj, proc])
    }

    fn rb_undefine_finalizer(obj: Value) -> Value {
        objspace_call("undefine_finalizer", &[obj])
    }

    /// `rb_set_end_proc(f, arg)`: run `f` at exit, which is `Kernel#at_exit`.
    fn rb_set_end_proc(f: unsafe extern "C" fn(Value), arg: Value) -> () {
        let (addr, a) = (f as usize, arg);
        let body = crate::rproc::ProcBuilder::from_rust(
            move |_recv, _args, _block| {
                // SAFETY: the caller's own function, in the loaded image.
                let f: unsafe extern "C" fn(Value) = unsafe { std::mem::transmute(addr) };
                super::jmp::protect(|| unsafe { f(a) })?;
                Ok(RubyValue::Nil)
            },
            RubyValue::Nil,
            0,
            false,
        )
        .build();
        crate::dispatch::send_value(
            &crate::dispatch::main_object(),
            Symbol::intern("at_exit"),
            &[],
            Some(RubyValue::Proc(body)),
        )?;
        Ok(())
    }

    fn rb_autoload_p(m: Value, id: Id) -> Value {
        let owner = unsafe { value_of(m) };
        let name = RubyValue::Symbol(symbol_of(id));
        to_value(&send(&owner, "autoload?", &[name])?)
    }

    /// `rb_autoload_load(mod, id)`: force the autoload NOW. Reading the
    /// constant is what triggers it, so that is the whole implementation.
    fn rb_autoload_load(m: Value, id: Id) -> Value {
        let owner = unsafe { value_of(m) };
        let name = RubyValue::Symbol(symbol_of(id));
        Ok(super::convert::boolean(send(&owner, "const_get", &[name]).is_ok()))
    }

    /// `rb_clear_constant_cache`: MRI invalidates its inline constant cache.
    /// zeo's constant reads go through a generation counter that every write
    /// already bumps, so an extension has nothing to invalidate by hand.
    fn rb_clear_constant_cache() -> () {
        Ok(())
    }

    fn rb_clear_constant_cache_for_id(_id: Id) -> () {
        Ok(())
    }

    // ---- TypedData -------------------------------------------------------

    /// `rb_typeddata_is_kind_of(v, type)`: is `v` a TypedData of this type,
    /// or of one that inherits from it? A plain pointer comparison would
    /// answer no for a subclass's data type, which is the bug this exists
    /// to avoid.
    fn rb_typeddata_is_kind_of(v: Value, ty: *const super::data::DataType) -> c_int {
        Ok(c_int::from(unsafe { super::data::is_kind_of(v, ty) }))
    }

    fn rb_typeddata_inherited_p(
        child: *const super::data::DataType,
        parent: *const super::data::DataType,
    ) -> c_int {
        Ok(c_int::from(super::data::type_inherits(child, parent)))
    }

    /// `rb_get_alloc_func(klass)`: the allocator `rb_define_alloc_func`
    /// installed. Answers null when the class has none, which is how a
    /// caller tells a C-allocated class from a plain Ruby one.
    fn rb_get_alloc_func(klass: Value) -> *const c_void {
        let cid = unsafe { super::object::as_class(klass)? };
        Ok(super::method::alloc_func_of(cid))
    }

    /// `rb_f_sprintf(argc, argv)`: `Kernel#sprintf`, where the first
    /// argument is the format and the rest are its arguments.
    fn rb_f_sprintf(argc: c_int, argv: *const Value) -> Value {
        let args = unsafe { args_of(argc, argv) };
        to_value(&crate::dispatch::send_value(
            &crate::dispatch::main_object(),
            Symbol::intern("sprintf"),
            &args,
            None,
        )?)
    }

    // ---- the entries that only raise -------------------------------------

    fn rb_notimplement() -> () {
        Err(crate::dispatch::raise_error(
            "NotImplementedError",
            "the platform does not support this method".into(),
        ))
    }

    fn rb_out_of_int(n: c_long) -> () {
        Err(crate::dispatch::raise_error(
            "RangeError",
            format!("integer {n} too big to convert to `int'"),
        ))
    }

    fn rb_unexpected_type(v: Value, want: c_int) -> () {
        let recv = unsafe { value_of(v) };
        Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "wrong argument type {} (expected T_{want})",
                crate::dispatch::class_name(recv.class_id()).unwrap_or("Object".into())
            ),
        ))
    }

    fn rb_scan_args_bad_format(fmt: *const c_char) -> () {
        Err(crate::dispatch::raise_error(
            "FatalError",
            format!("bad scan arg format: {}", unsafe { cstr(fmt) }),
        ))
    }

    fn rb_scan_args_length_mismatch(fmt: *const c_char, n: c_int) -> () {
        Err(crate::dispatch::raise_error(
            "FatalError",
            format!("bad scan arg format: {} ({n})", unsafe { cstr(fmt) }),
        ))
    }

    /// `rb_varargs_bad_length(passed, expected)`: a `rb_funcall` whose
    /// argument count does not match what the macro counted. MRI aborts;
    /// raising names the extension's own bug and lets a test see it.
    fn rb_varargs_bad_length(passed: c_int, expected: c_int) -> c_int {
        Err(crate::dispatch::raise_error(
            "ArgumentError",
            format!("wrong number of arguments ({passed} for {expected})"),
        ))
    }

    /// `rb_debug_rstring_null_ptr(func)`: MRI's report for an
    /// `RSTRING_PTR` that answered null. zeo's never does -- the pin always
    /// allocates -- so reaching this means the extension corrupted something.
    fn rb_debug_rstring_null_ptr(func: *const c_char) -> () {
        eprintln!(
            "zeo: {} got a null RSTRING_PTR, which zeo never produces",
            unsafe { cstr(func) }
        );
        Ok(())
    }

    // ---- the process and signal odds -------------------------------------

    fn rb_waitpid(pid: i32, status: *mut c_int, flags: c_int) -> i32 {
        let out = send(
            &class_named("Process")?,
            "waitpid2",
            &[RubyValue::Int(i64::from(pid)), RubyValue::Int(i64::from(flags))],
        )?;
        let RubyValue::Array(pair) = &out else {
            return Ok(-1);
        };
        let g = pair.lock();
        let got = match g.first() {
            Some(RubyValue::Int(n)) => *n as i32,
            _ => -1,
        };
        if !status.is_null() {
            let code = match g.get(1) {
                Some(st) => match send(st, "to_i", &[]) {
                    Ok(RubyValue::Int(n)) => n as c_int,
                    _ => 0,
                },
                None => 0,
            };
            // SAFETY: the caller's own `int`.
            unsafe { status.write(code) };
        }
        Ok(got)
    }

    fn rb_syswait(pid: i32) -> () {
        unsafe { rb_waitpid(pid, std::ptr::null_mut(), 0) };
        Ok(())
    }

    fn rb_detach_process(pid: i32) -> Value {
        to_value(&send(
            &class_named("Process")?,
            "detach",
            &[RubyValue::Int(i64::from(pid))],
        )?)
    }

    fn rb_process_status_wait(pid: i32, flags: c_int) -> Value {
        to_value(&send(
            &class_named("Process")?,
            "wait2",
            &[RubyValue::Int(i64::from(pid)), RubyValue::Int(i64::from(flags))],
        )?)
    }

    fn rb_spawn(argc: c_int, argv: *const Value) -> i32 {
        let args = unsafe { args_of(argc, argv) };
        match send(&class_named("Process")?, "spawn", &args)? {
            RubyValue::Int(n) => Ok(n as i32),
            other => Err(wrong_type(&other, "Integer")),
        }
    }

    /// `rb_spawn_err(argc, argv, errmsg, buflen)`: the same, writing the
    /// failure into the caller's buffer rather than raising. Answers -1 on
    /// failure, which is what the caller tests.
    fn rb_spawn_err(argc: c_int, argv: *const Value, errmsg: *mut c_char, buflen: usize) -> i32 {
        match unsafe { std::panic::catch_unwind(|| rb_spawn(argc, argv)) } {
            Ok(pid) if pid >= 0 => Ok(pid),
            _ => {
                write_err(errmsg, buflen, "spawn failed");
                Ok(-1)
            }
        }
    }

    fn rb_proc_exec(cmd: *const c_char) -> c_int {
        let text = a_string(&unsafe { cstr(cmd) });
        crate::dispatch::send_value(
            &crate::dispatch::main_object(),
            Symbol::intern("exec"),
            &[text],
            None,
        )?;
        Ok(-1)
    }

    fn rb_proc_times(v: Value) -> Value {
        let _ = unsafe { value_of(v) };
        to_value(&send(&class_named("Process")?, "times", &[])?)
    }

    fn rb_reset_random_seed() -> () {
        let random = class_named("Random")?;
        send(&random, "srand", &[])?;
        Ok(())
    }

    /// `ruby_signal_name(sig)`: `"INT"` for 2. MRI answers the name WITHOUT
    /// the `SIG` prefix, and a caller printing `SIG%s` would otherwise
    /// produce `SIGSIGINT`.
    fn ruby_signal_name(sig: c_int) -> *const c_char {
        let list = send(&class_named("Signal")?, "list", &[])?;
        let RubyValue::Hash(h) = &list else {
            return Ok(std::ptr::null());
        };
        let found = h
            .lock()
            .iter()
            .find(|(_, (_, v))| matches!(v, RubyValue::Int(n) if *n == i64::from(sig)))
            .map(|(_, (k, _))| k.to_display_string());
        Ok(match found {
            Some(name) => super::symbol::cstr_for_owned(&name),
            None => std::ptr::null(),
        })
    }

    /// `ruby_default_signal(sig)`: re-raise with the default handler, which
    /// is how a Ruby program dies from `SIGINT` with the right exit status.
    fn ruby_default_signal(sig: c_int) -> () {
        // SAFETY: restoring the default disposition and re-raising is the
        // documented way to exit with a signal's own status.
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
        Ok(())
    }

    fn ruby_native_thread_p() -> c_int {
        // Every thread that can reach here came through zeo's own runtime.
        Ok(1)
    }

    /// `ruby_stack_check()`: is the machine stack nearly full? MRI answers
    /// from its own recorded stack bounds, which it uses to raise
    /// `SystemStackError` before a real overflow. zeo's overflow guard is
    /// the platform's guard page, so there is no headroom figure to
    /// report -- and 0 is the answer that lets the caller keep going.
    fn ruby_stack_check() -> c_int {
        Ok(0)
    }

    fn ruby_stack_length(base: *mut *mut Value) -> usize {
        if !base.is_null() {
            // SAFETY: the caller's own out-pointer.
            unsafe { base.write(std::ptr::null_mut()) };
        }
        Ok(0)
    }

    fn ruby_strtoul(p: *const c_char, end: *mut *mut c_char, base: c_int) -> usize {
        let text = unsafe { cstr(p) };
        let radix = if base == 0 { 10 } else { base.max(2) as u32 };
        let digits = text.trim_start();
        let stop = digits
            .find(|c: char| !c.is_digit(radix))
            .unwrap_or(digits.len());
        if !end.is_null() {
            let consumed = text.len() - digits.len() + stop;
            // SAFETY: the caller's own `char **`.
            unsafe { end.write(p.cast_mut().add(consumed)) };
        }
        Ok(usize::from_str_radix(&digits[..stop], radix).unwrap_or(0))
    }

    // ---- globs -----------------------------------------------------------

    /// `ruby_glob(pattern, flags, f, arg)`: call `f` once per match. The
    /// callback answers 0 to keep going, and a nonzero answer becomes this
    /// function's own answer -- which is how a caller aborts a walk.
    fn ruby_glob(
        pattern: *const c_char,
        _flags: c_int,
        f: unsafe extern "C" fn(*const c_char, Value, *mut c_void) -> c_int,
        arg: Value,
    ) -> c_int {
        glob_walk(&unsafe { cstr(pattern) }, f, arg)
    }

    /// `ruby_brace_glob`: the same, and `Dir.glob` already expands `{a,b}`.
    fn ruby_brace_glob(
        pattern: *const c_char,
        _flags: c_int,
        f: unsafe extern "C" fn(*const c_char, Value, *mut c_void) -> c_int,
        arg: Value,
    ) -> c_int {
        glob_walk(&unsafe { cstr(pattern) }, f, arg)
    }

    /// `rb_glob(pattern, f, arg)`: the void-returning spelling.
    fn rb_glob(
        pattern: *const c_char,
        f: unsafe extern "C" fn(*const c_char, Value, *mut c_void),
        arg: Value,
    ) -> () {
        for path in glob_paths(&unsafe { cstr(pattern) })? {
            let c = super::symbol::cstr_for_owned(&path);
            // SAFETY: the caller's own callback, on a NUL-terminated path.
            super::jmp::protect(|| unsafe { f(c, arg, std::ptr::null_mut()) })?;
        }
        Ok(())
    }

    // ---- Enumerator::ArithmeticSequence -----------------------------------

    /// `rb_arithmetic_sequence_extract(obj, &out)`: fill a
    /// `rb_arithmetic_sequence_components_t` from a sequence or a Range.
    /// Answers 1 when it filled one.
    fn rb_arithmetic_sequence_extract(v: Value, out: *mut ArithSeq) -> c_int {
        let seq = unsafe { value_of(v) };
        let name = crate::dispatch::class_name(seq.class_id()).unwrap_or_default();
        let is_seq = name.ends_with("ArithmeticSequence");
        if !is_seq && name != "Range" {
            return Ok(0);
        }
        let begin = send(&seq, "begin", &[])?;
        let end = send(&seq, "end", &[])?;
        let excl = send(&seq, "exclude_end?", &[])?;
        let step = if is_seq {
            send(&seq, "step", &[])?
        } else {
            RubyValue::Int(1)
        };
        if !out.is_null() {
            // SAFETY: the caller's own struct.
            unsafe {
                out.write(ArithSeq {
                    begin: to_value(&begin)?,
                    end: to_value(&end)?,
                    step: to_value(&step)?,
                    exclude_end: c_int::from(matches!(excl, RubyValue::Bool(true))),
                });
            }
        }
        Ok(1)
    }

    /// `rb_arithmetic_sequence_beg_len_step(obj, &beg, &len, &step, alen,
    /// err)`: the clamped bounds, the way `rb_range_beg_len` answers them.
    fn rb_arithmetic_sequence_beg_len_step(
        v: Value,
        beg: *mut c_long,
        len: *mut c_long,
        step: *mut c_long,
        alen: c_long,
        err: c_int,
    ) -> Value {
        let mut seq = ArithSeq {
            begin: value::Q_NIL,
            end: value::Q_NIL,
            step: value::Q_NIL,
            exclude_end: 0,
        };
        // SAFETY: a struct this frame owns.
        if unsafe { rb_arithmetic_sequence_extract(v, &raw mut seq) } == 0 {
            return Ok(value::Q_FALSE);
        }
        let range = unsafe {
            super::builtins::rb_range_new(seq.begin, seq.end, seq.exclude_end)
        };
        let out = unsafe { super::builtins::rb_range_beg_len(range, beg, len, alen, err) };
        if !step.is_null() {
            let s = match unsafe { value_of(seq.step) } {
                RubyValue::Int(n) => n as c_long,
                _ => 1,
            };
            // SAFETY: the caller's own `long`.
            unsafe { step.write(s) };
        }
        Ok(out)
    }

    // ---- constants as an st_table ----------------------------------------

    /// `rb_mod_const_of(mod, data)`: add the module's constants -- its own
    /// AND its ancestors' -- to an `st_table`, keyed by `ID`. A null `data`
    /// starts a fresh table, which is how the first call in a chain spells
    /// it.
    fn rb_mod_const_of(m: Value, data: *mut c_void) -> *mut c_void {
        collect_constants(m, data, false)
    }

    /// `rb_mod_const_at(mod, data)`: the same, the module's OWN only. The
    /// difference is exactly `Module#constants(false)`.
    fn rb_mod_const_at(m: Value, data: *mut c_void) -> *mut c_void {
        collect_constants(m, data, true)
    }

    /// `rb_const_list(data)`: turn that table into the Array of Symbols
    /// `Module#constants` answers, and free the table -- MRI frees it here
    /// too, which is why the caller must not use it afterwards.
    fn rb_const_list(data: *mut c_void) -> Value {
        let tbl = data.cast::<super::st::StTable>();
        let mut out: Vec<RubyValue> = Vec::new();
        for (k, _) in super::st::rows_of(tbl) {
            out.push(RubyValue::Symbol(symbol_of(k as Id)));
        }
        // SAFETY: the table came from `rb_mod_const_of`, which made it here.
        unsafe { super::st::rb_st_free_table(tbl) };
        to_value(&RubyValue::Array(crate::value::collections::array_new(out)))
    }

    // ---- waiting on a descriptor -----------------------------------------

    /// `rb_thread_wait_fd(fd)`: block until readable, releasing the GVL.
    /// Through `IO.select`, so the wait is interruptible by `Thread#kill`
    /// exactly as a Ruby-level select is.
    fn rb_thread_wait_fd(fd: c_int) -> c_int {
        select_on(fd, false, None)
    }

    fn rb_thread_fd_writable(fd: c_int) -> c_int {
        select_on(fd, true, None)
    }

    /// `rb_thread_fd_select(max, r, w, e, timeout)`: MRI's own `select`
    /// wrapper over `rb_fdset_t`, whose layout is the platform's `fd_set`.
    /// This is a raw `select(2)` -- the GVL is not held across it, because
    /// zeo releases it around any blocking syscall.
    fn rb_thread_fd_select(
        max: c_int,
        r: *mut c_void,
        w: *mut c_void,
        e: *mut c_void,
        timeout: *mut super::builtins::Timeval,
    ) -> c_int {
        // SAFETY: the caller's own `fd_set`s and `struct timeval`, in the
        // shapes `select(2)` names.
        let out = unsafe {
            libc::select(
                max,
                r.cast(),
                w.cast(),
                e.cast(),
                timeout.cast(),
            )
        };
        if out < 0 {
            return Err(crate::builtins::file::raise_bare_errno(
                &std::io::Error::last_os_error(),
            ));
        }
        Ok(out)
    }

    /// `rb_thread_wait_for(tv)`: sleep for a duration, interruptibly.
    fn rb_thread_wait_for(tv: super::builtins::Timeval) -> () {
        let secs = tv.tv_sec as f64 + f64::from(tv.tv_usec) / 1e6;
        crate::dispatch::send_value(
            &crate::dispatch::main_object(),
            Symbol::intern("sleep"),
            &[RubyValue::Float(secs.max(0.0))],
            None,
        )?;
        Ok(())
    }

    /// `rb_thread_fd_close(fd)`: MRI wakes every thread blocked on the
    /// descriptor so their `select` returns before the close. zeo's waits
    /// are `IO.select` calls that already return on close, so there is no
    /// separate wake to send.
    fn rb_thread_fd_close(_fd: c_int) -> () {
        Ok(())
    }

    /// `rb_close_before_exec(lowfd, maxhint, noclose_fds)`: close every
    /// descriptor above `lowfd` so an `exec` does not inherit it. zeo sets
    /// `FD_CLOEXEC` on everything it opens, so this only has to cover
    /// descriptors that came from elsewhere.
    fn rb_close_before_exec(lowfd: c_int, maxhint: c_int, _noclose: Value) -> () {
        for fd in lowfd.max(0)..=maxhint.max(lowfd) {
            // SAFETY: setting the flag is safe on any descriptor, open or
            // not -- a closed one answers EBADF and is ignored.
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        }
        Ok(())
    }

    // ---- the two `VALUE *` globals ---------------------------------------

    /// `rb_ruby_verbose_ptr()` and `rb_ruby_debug_ptr()`: the ADDRESS of
    /// `$VERBOSE` and `$DEBUG`, which MRI's `RTEST(*rb_ruby_verbose_ptr())`
    /// reads directly.
    ///
    /// zeo's globals live in a table, not at a fixed address, so each is
    /// mirrored into a static word that is refreshed on every call. A read
    /// is therefore current; a WRITE through the pointer does not reach the
    /// Ruby global, and `$VERBOSE = x` is the spelling that does.
    fn rb_ruby_verbose_ptr() -> *mut Value {
        Ok(mirror(&VERBOSE_MIRROR, "$VERBOSE"))
    }

    fn rb_ruby_debug_ptr() -> *mut Value {
        Ok(mirror(&DEBUG_MIRROR, "$DEBUG"))
    }

    // ---- symbols in another extension ------------------------------------

    /// `rb_ext_resolve_symbol(feature, name)`: find a symbol another loaded
    /// extension exported. `RTLD_DEFAULT` searches every loaded image, which
    /// is what makes this work at all -- and it is the same tier zeo's FFI
    /// already uses.
    fn rb_ext_resolve_symbol(_feature: *const c_char, name: *const c_char) -> *mut c_void {
        let Ok(sym) = std::ffi::CString::new(unsafe { cstr(name) }) else {
            return Ok(std::ptr::null_mut());
        };
        // SAFETY: a NUL-terminated symbol name; a miss answers null.
        Ok(unsafe { libc::dlsym(libc::RTLD_DEFAULT, sym.as_ptr()) })
    }

    /// `rb_ext_ractor_safe(flag)`: the extension declares it may run in a
    /// non-main Ractor. zeo's Ractors do not run C extensions at all yet, so
    /// the flag has nothing to gate -- and recording it would suggest a
    /// guarantee that is not tested.
    fn rb_ext_ractor_safe(_safe: bool) -> () {
        Ok(())
    }

    // ---- the aborts ------------------------------------------------------

    /// `rb_assert_failure`: a failed `RUBY_ASSERT`. It is a bug in the
    /// extension by construction, so it aborts as `rb_bug` does rather than
    /// raising something a `rescue` could swallow.
    fn rb_assert_failure(
        file: *const c_char,
        line: c_int,
        name: *const c_char,
        expr: *const c_char,
    ) -> () {
        eprintln!(
            "zeo: {}:{line}:{}: assertion failed: {}",
            unsafe { cstr(file) },
            unsafe { cstr(name) },
            unsafe { cstr(expr) }
        );
        std::process::abort()
    }

    fn rb_bug_errno(msg: *const c_char, code: c_int) -> () {
        let text = std::io::Error::from_raw_os_error(code).to_string();
        eprintln!("zeo: a C extension called rb_bug_errno: {} ({text})", unsafe {
            cstr(msg)
        });
        std::process::abort()
    }

    fn ruby_malloc_size_overflow(count: usize, size: usize) -> () {
        Err(crate::dispatch::raise_error(
            "NoMemoryError",
            format!("malloc: possible integer overflow ({count} * {size})"),
        ))
    }

    fn ruby_malloc_add_size_overflow(a: usize, b: usize) -> () {
        Err(crate::dispatch::raise_error(
            "NoMemoryError",
            format!("malloc: possible integer overflow ({a} + {b})"),
        ))
    }

    // ---- Bignum magnitude ------------------------------------------------

    /// `rb_absint_numwords(v, wordbits, &nlz)`: how many words of
    /// `wordbits` bits the magnitude needs, and how many leading bits of the
    /// top word are zero. `rb_integer_pack`'s caller sizes its buffer from
    /// this, so an answer one too small is a truncated number.
    fn rb_absint_numwords(v: Value, wordbits: usize, nlz: *mut usize) -> usize {
        let n = unsafe { super::numeric::abs_bits(v)? };
        if wordbits == 0 {
            return Ok(0);
        }
        let words = n.div_ceil(wordbits).max(usize::from(n > 0));
        if !nlz.is_null() {
            let used = if words == 0 { 0 } else { n - (words - 1) * wordbits };
            // SAFETY: the caller's own `size_t`.
            unsafe { nlz.write(wordbits.saturating_sub(used)) };
        }
        Ok(words)
    }

    /// `rb_absint_singlebit_p(v)`: is the magnitude a power of two? An
    /// extension uses it to take a shift instead of a divide.
    fn rb_absint_singlebit_p(v: Value) -> c_int {
        Ok(c_int::from(unsafe { super::numeric::abs_is_power_of_two(v)? }))
    }
}

/// `rb_data_define`'s worker, from `csrc/cext_va.c`.
///
/// # Safety
///
/// `members` must name `n` NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_cext_data_define(
    super_class: Value,
    members: *const *const c_char,
    n: c_int,
) -> Value {
    let out = (|| -> Result<Value, Signal> {
        let base = match unsafe { value_of(super_class) } {
            RubyValue::Class(_) => unsafe { value_of(super_class) },
            _ => class_named("Data")?,
        };
        let args = unsafe { member_symbols(members, n) };
        to_value(&send(&base, "define", &args)?)
    })();
    match out {
        Ok(v) => v,
        Err(sig) => super::jmp::raise(sig),
    }
}

/// `rb_struct_define_without_accessor`'s worker.
///
/// MRI builds a Struct class with the members but WITHOUT the reader and
/// writer methods, so the extension can define its own. zeo's `Struct.new`
/// always makes them, and `undef_method` after the fact is what leaves the
/// same surface.
///
/// # Safety
///
/// `members` must name `n` NUL-terminated strings, and `name` must be
/// NUL-terminated or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_cext_struct_define_noaccessor(
    outer: Value,
    name: *const c_char,
    _super_class: Value,
    members: *const *const c_char,
    n: c_int,
) -> Value {
    let out = (|| -> Result<Value, Signal> {
        let args = unsafe { member_symbols(members, n) };
        let cls = send(&class_named("Struct")?, "new", &args)?;
        for m in &args {
            let RubyValue::Symbol(s) = m else { continue };
            let setter = RubyValue::Symbol(Symbol::intern(&format!("{}=", s.name_str())));
            let _ = send(&cls, "undef_method", &[m.clone(), setter]);
        }
        let leaf = unsafe { cstr(name) };
        if !leaf.is_empty()
            && let RubyValue::Class(cid) = &cls
        {
            let home = if outer == 0 || outer == value::Q_NIL {
                zeo_abi::OBJECT_CLASS
            } else {
                unsafe { super::object::as_class(outer)? }
            };
            let full = match crate::dispatch::class_name(home) {
                Some(o) if home != zeo_abi::OBJECT_CLASS => format!("{o}::{leaf}"),
                _ => leaf.clone(),
            };
            crate::runtime_meta::name_runtime_class_if_anonymous(*cid, &full);
            crate::constants::const_set(home.0, &leaf, cls.clone());
        }
        to_value(&cls)
    })();
    match out {
        Ok(v) => v,
        Err(sig) => super::jmp::raise(sig),
    }
}

/// A NUL-terminated member list as Symbols.
///
/// # Safety
///
/// `members` must name `n` NUL-terminated strings.
unsafe fn member_symbols(members: *const *const c_char, n: c_int) -> Vec<RubyValue> {
    (0..n.max(0))
        .map(|i| {
            // SAFETY: the caller's contract.
            let name = unsafe { cstr(members.offset(i as isize).read()) };
            RubyValue::Symbol(Symbol::intern(&name))
        })
        .collect()
}

/// `rb_arithmetic_sequence_components_t`, field for field.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArithSeq {
    pub begin: Value,
    pub end: Value,
    pub step: Value,
    pub exclude_end: c_int,
}

fn no_conversion(recv: &RubyValue, tname: &str) -> Signal {
    crate::dispatch::raise_error(
        "TypeError",
        format!(
            "can't convert {} into {tname}",
            crate::dispatch::class_name(recv.class_id()).unwrap_or("Object".into())
        ),
    )
}

fn cmperr(a: Value, b: Value) -> Signal {
    let (x, y) = (unsafe { value_of(a) }, unsafe { value_of(b) });
    crate::dispatch::raise_error(
        "ArgumentError",
        format!(
            "comparison of {} with {} failed",
            crate::dispatch::class_name(x.class_id()).unwrap_or("Object".into()),
            y.to_display_string()
        ),
    )
}

/// Bytes from outside the process. Every boundary encoding zeo builds
/// extensions for is UTF-8, so all six entries land here.
fn outside_str(p: *const c_char, len: c_long) -> Result<Value, Signal> {
    let bytes = unsafe { super::string::borrow_bytes(p, len) };
    to_value(&RubyValue::Str(crate::string_from_bytes(
        bytes,
        crate::encoding::UTF_8,
    )))
}

fn objspace_call(meth: &str, args: &[Value]) -> Result<Value, Signal> {
    let os = class_named("ObjectSpace")?;
    let vals: Vec<RubyValue> = args.iter().map(|a| unsafe { value_of(*a) }).collect();
    to_value(&send(&os, meth, &vals)?)
}

fn write_err(buf: *mut c_char, cap: usize, msg: &str) {
    if buf.is_null() || cap == 0 {
        return;
    }
    let bytes = msg.as_bytes();
    let n = bytes.len().min(cap - 1);
    // SAFETY: the caller promised `cap` writable bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf.cast::<u8>(), n);
        buf.add(n).write(0);
    }
}

/// `$VERBOSE` and `$DEBUG` mirrored into a fixed word, because MRI's API
/// hands out the ADDRESS of each. Refreshed on every read; see the two
/// entries for why a write through the pointer does not travel.
static VERBOSE_MIRROR: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(value::Q_NIL);
static DEBUG_MIRROR: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(value::Q_NIL);

fn mirror(cell: &'static std::sync::atomic::AtomicUsize, name: &str) -> *mut Value {
    let held = crate::globals::global_get(0, name);
    let raw = super::value::immediate_of(&held).unwrap_or(value::Q_NIL);
    cell.store(raw, std::sync::atomic::Ordering::Relaxed);
    // The atomic and a `VALUE` are the same word, which is what lets C read
    // it as one -- the same trick `cext/stubs.rs` uses for every global.
    std::ptr::from_ref(cell).cast::<Value>().cast_mut()
}

/// `Module#constants` into an `st_table` keyed by `ID`, as MRI's two
/// `rb_mod_const_*` entries build one.
fn collect_constants(m: Value, data: *mut c_void, own_only: bool) -> Result<*mut c_void, Signal> {
    let owner = unsafe { value_of(m) };
    let tbl = if data.is_null() {
        // SAFETY: no arguments; the table is the caller's to free through
        // `rb_const_list`.
        unsafe { super::st::rb_st_init_numtable() }
    } else {
        data.cast::<super::st::StTable>()
    };
    let names = send(&owner, "constants", &[RubyValue::Bool(!own_only)])?;
    if let RubyValue::Array(a) = names {
        for name in a.lock().iter() {
            let RubyValue::Symbol(s) = name else { continue };
            // SAFETY: a table this function owns or the caller passed in.
            unsafe { super::st::rb_st_insert(tbl, s.to_u32() as usize, 0) };
        }
    }
    Ok(tbl.cast())
}

fn glob_paths(pattern: &str) -> Result<Vec<String>, Signal> {
    let dir = class_named("Dir")?;
    let out = send(&dir, "glob", &[a_string(pattern)])?;
    Ok(match out {
        RubyValue::Array(a) => a.lock().iter().map(RubyValue::to_display_string).collect(),
        _ => Vec::new(),
    })
}

/// Wait for one descriptor through `IO.select`, so the wait is
/// interruptible. Answers 0, which is what MRI's own entries answer.
fn select_on(fd: c_int, writable: bool, timeout: Option<f64>) -> Result<c_int, Signal> {
    let io = class_named("IO")?;
    let cls = class_named("IO")?;
    let target = send(&cls, "for_fd", &[RubyValue::Int(i64::from(fd))])?;
    let list = RubyValue::Array(crate::value::collections::array_new(vec![target]));
    let none = RubyValue::Nil;
    let args = if writable {
        vec![
            none.clone(),
            list,
            none,
            timeout.map_or(RubyValue::Nil, RubyValue::Float),
        ]
    } else {
        vec![
            list,
            none.clone(),
            none,
            timeout.map_or(RubyValue::Nil, RubyValue::Float),
        ]
    };
    send(&io, "select", &args)?;
    Ok(0)
}

fn glob_walk(
    pattern: &str,
    f: unsafe extern "C" fn(*const c_char, Value, *mut c_void) -> c_int,
    arg: Value,
) -> Result<c_int, Signal> {
    for path in glob_paths(pattern)? {
        let c = super::symbol::cstr_for_owned(&path);
        // SAFETY: the caller's own callback, on a NUL-terminated path.
        let verdict = super::jmp::protect(|| unsafe { f(c, arg, std::ptr::null_mut()) })?;
        if verdict != 0 {
            return Ok(verdict);
        }
    }
    Ok(0)
}
