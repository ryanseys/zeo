//! `Kernel` -- the module every "universal" method actually belongs to
//! (CRuby's `Object` owns ZERO instance methods; object.c defines these on
//! `rb_mKernel`). Reached on every receiver through the MRO walk, since
//! every chain ends `..., Object, Kernel, BasicObject`.
//!
//! Rows migrated from `send`/`send_value`'s old hardwired universal arms:
//! `class`, `dup`/`clone`, `hash`, `to_s`/`inspect`, `is_a?`/`kind_of?`,
//! `instance_of?` -- plus the CRuby-owned additions `nil?`, `itself`,
//! `frozen?`/`freeze`, `eql?`, `===`, `respond_to?`, `tap`, `then`.
//! `Kernel#<=>` (identity-or-nil default) is deliberately ABSENT until the
//! numeric operator rows move into `integer.rs`/`float.rs` (stage C) -- it
//! would shadow the post-walk numeric `<=>` today.

mod convert;
mod exit;
mod io;
mod load;
mod random;

use crate::builtins::inherited_row;
use crate::builtins::{arg_error, block_or_enum, local_jump_error, need_block, type_error};
use crate::{RubyValue, Signal, Symbol};
use zeo_macros::ruby_module;

use convert::*;
use io::*;

pub(crate) use convert::{
    array_impl, complex_impl, float_impl, hash_impl, integer_impl, rational_impl, string_impl,
};
pub(crate) use exit::build_raise_exception;
pub use exit::{kernel_abort, kernel_exit, kernel_exit_bang, system_exit_status};
pub(crate) use io::output_separator;
pub use io::{
    kernel_format, kernel_p, kernel_pp, kernel_print, kernel_printf, kernel_puts, kernel_warn,
};
pub(crate) use load::{
    dynamic_require, dynamic_require_relative, feature_already_loaded, missing_feature_error,
};
pub(crate) use random::{prng_limited, rand_impl, srand_impl};

/// `Object` as a receiver value -- what a receiverless `autoload` registers on.
fn object_value() -> RubyValue {
    RubyValue::Class(zeo_abi::OBJECT_CLASS)
}

/// A `Vec<Symbol>` as a Ruby Array of Symbols -- reflection's return shape.
fn syms_to_array(names: Vec<Symbol>) -> RubyValue {
    RubyValue::Array(crate::array_new(
        names.into_iter().map(RubyValue::Symbol).collect(),
    ))
}

/// Order-preserving dedup for a combined symbol list (each of the two source
/// lists is already internally deduped; this merges them).
fn dedup_syms(names: Vec<Symbol>) -> Vec<Symbol> {
    let mut seen = std::collections::HashSet::new();
    names.into_iter().filter(|s| seen.insert(*s)).collect()
}

/// The shared reading of reflection's optional `inherit` argument: absent
/// means true, and `false`/`nil` are the only ways to narrow.
fn truthy_arg(arg: Option<&RubyValue>) -> bool {
    !matches!(arg, Some(RubyValue::Bool(false)) | Some(RubyValue::Nil))
}

/// The methods installed on this value BY IDENTITY -- what both
/// `singleton_methods(false)` and `methods(false)` report for a receiver that
/// is not a class.
/// The per-object singleton names whose visibility satisfies `keep`. An
/// unmarked row is public, which is what `def obj.x` writes.
fn singleton_names_where(
    recv: &RubyValue,
    keep: impl Fn(crate::dispatch::MethodVisibility) -> bool,
) -> Vec<Symbol> {
    crate::runtime_meta::singleton_method_names(recv)
        .into_iter()
        .filter(|&n| {
            keep(
                crate::runtime_meta::singleton_visibility(recv, n)
                    .unwrap_or(crate::dispatch::MethodVisibility::Public),
            )
        })
        .collect()
}

fn own_singleton_names(recv: &RubyValue) -> Vec<Symbol> {
    // `main` carries eight singletons in CRuby, of which only `to_s` and
    // `inspect` are PUBLIC -- the other six (`define_method`, `include`,
    // `private`, `public`, `ruby2_keywords`, `using`) are private and so do
    // not show here. zeo installs no singleton on main at all, because doing
    // it at startup would mark the runtime-overlay maps live for every
    // program, so the two public names are named here instead.
    // A `def self.x` written at the top level lands on main's singleton like
    // any other per-object method, so the two named here are a FLOOR and not
    // the whole list.
    if let RubyValue::Object(o) = recv
        && crate::dispatch::is_main_object(o)
    {
        let mut names = vec![Symbol::intern("inspect"), Symbol::intern("to_s")];
        names.extend(singleton_names_where(recv, |vis| {
            vis != crate::dispatch::MethodVisibility::Private
        }));
        return dedup_syms(names);
    }
    match recv {
        RubyValue::Class(cid) => crate::dispatch::public_class_method_names(*cid, false),
        // Both report the PUBLIC surface. `def obj.x` is public as written, so
        // the filter only ever removes a name the object was explicitly told
        // to hide -- a `private` cursor over a `class << obj` body, a
        // `private :x` sent to its singleton class, or an `extend` that copied
        // a `module_function` name in.
        _ => singleton_names_where(recv, |vis| {
            vis != crate::dispatch::MethodVisibility::Private
        }),
    }
}

/// A stable per-identity Integer. Objects use their `Arc` pointer; immediates
/// use CRuby's fixed/derived shapes (Integers `2n+1`, nil/true/false their
/// reserved slots). Strings/Arrays/Hashes use their cell pointer -- identity,
/// not content.
///
/// Shared, because CRuby splits the two names across two classes:
/// `Kernel#object_id` and `BasicObject#__id__`.
pub(crate) fn object_id_of(recv: &RubyValue) -> RubyValue {
    RubyValue::Int(match recv {
        RubyValue::Int(i) => i.wrapping_mul(2).wrapping_add(1),
        // CRuby 4.0.6's fixed immediate ids: nil 4, true 20, false 0.
        RubyValue::Nil => 4,
        RubyValue::Bool(true) => 20,
        RubyValue::Bool(false) => 0,
        RubyValue::Object(o) => std::sync::Arc::as_ptr(o) as *const () as i64,
        RubyValue::Str(s) => std::sync::Arc::as_ptr(s) as i64,
        RubyValue::Array(a) => std::sync::Arc::as_ptr(a) as i64,
        RubyValue::Hash(h) => std::sync::Arc::as_ptr(h) as i64,
        RubyValue::Range(r) => std::sync::Arc::as_ptr(r) as i64,
        RubyValue::Symbol(s) => 0x1000_0000_0000 + i64::from(s.to_u32()),
        // A class/module IS its id, so derive from that. The fallback below
        // cannot serve here: a `Class` is a bare `ClassId`, so `recv` points at
        // whatever temporary slot the caller built, and every class in a loop
        // reads back the same address -- `Array.object_id == Hash.object_id`.
        RubyValue::Class(cid) => 0x2000_0000_0000 + i64::from(cid.0),
        // The remaining kinds get a per-call address-ish value -- a documented
        // approximation (identity comparison via object_id on them is rare).
        // It holds only while the values sit in distinct slots; anything with
        // a stable identity of its own should get an arm above instead.
        _ => recv as *const _ as i64,
    })
}

ruby_module! {
    Kernel = zeo_abi::KERNEL_CLASS;

    // `send`/`public_send` are KERNEL's, not BasicObject's (vm_eval.c:2961,
    // :2963) -- which is what makes them absent on a blank-slate receiver
    // while `__send__` still works there. They share BasicObject's one
    // implementation, as `rb_f_send` does in CRuby.
    //
    // `send` is deliberately visibility-blind. `public_send` is not: its gate
    // is `dispatch::send_value_public_in`. The emitter has no call-site
    // fold for that gate and reaches this row always -- so the rule
    // lives HERE.
    def "send"(recv, *args, &block) {
        crate::builtins::basic_object::dynamic_send(recv, args, block)
    }
    def "public_send"(recv, *args, &block) {
        crate::builtins::basic_object::public_dynamic_send(recv, args, block)
    }

    // The print family as REAL Kernel methods (Path 2): `obj.send(:puts,
    // ...)`, `self.puts` on `main`, and any dynamic dispatch reach these;
    // the receiver is ignored, exactly like CRuby's private Kernel#puts.
    module_function def "puts"(_recv, *args, &_block) {
        kernel_puts(args)
    }
    module_function def "print"(_recv, *args, &_block) {
        kernel_print(args)
    }
    module_function def "p"(_recv, *args, &_block) {
        kernel_p(args)
    }
    // `Kernel#open(path, mode = "r")` -- opens a File (the `"|command"` pipe
    // form is out of scope); delegates to `File.open` so the block-closes-file
    // contract and mode handling are shared, never divergent.
    module_function def "open"(_recv, *args, &block) {
        // `open` is IO's row, which File inherits (CRuby defines it on IO
        // alone). Handing it a File receiver is what selects the path form.
        crate::builtins::io::lookup_class("open").unwrap()(
            &RubyValue::Class(zeo_abi::FILE_CLASS),
            args,
            block,
        )
    }
    // `require`/`require_relative`/`load` in a NON-resolvable position reach
    // here at runtime. Whole-program AOT already spliced every
    // compile-time-resolvable require, so the only calls that land here are
    // genuinely dynamic (a computed path, or `load`). zeo has no runtime Ruby
    // loader, so an actually-executed dynamic load raises CRuby's LoadError
    // shape -- honest, and rescuable by `begin; require dyn; rescue LoadError`
    // -- rather than a silent no-op. A non-String-convertible argument raises
    // the same TypeError CRuby's path coercion does.
    // `load` takes a second `wrap` argument and is a C function that discarded
    // its signature, so it reports -1 where `require` reports 1. zeo resolves
    // all three the same way, so `load` delegates.
    module_function def "load" cfunc (recv, arg1, _arg2?) {
        // `load` RE-EXECUTES, always, and names an exact file -- so the disk
        // tier comes first and asks for no `.rb` to be appended. A feature
        // the compiler spliced has no file to re-read, so it falls through
        // to `require`'s once-only answer.
        let path = crate::builtins::convert::to_rstr(arg1)?.lock().to_utf8_lossy().into_owned();
        if let Some(result) = crate::features::load_from_disk(&path, 0, true) {
            return result.map(RubyValue::Bool);
        }
        // `rb_load_internal` dlopens a compiled extension too, and `Init_` is
        // not re-runnable, so a second `load` of one answers false rather
        // than defining its classes twice.
        if let Some(result) = crate::features::load_native_from_disk(&path, 0) {
            return result.map(RubyValue::Bool);
        }
        require_feature(recv, std::slice::from_ref(arg1), None)
    }
    // Written as two defs rather than one `module_function`: CRuby defines
    // `Kernel#require` and `Kernel.require` separately, and only the instance
    // one reports its parameter name.
    private def "require" params "path" as require_feature (_recv, arg1) {
        dynamic_require(arg1)
    }
    def self."require"(recv, arg1) {
        require_feature(recv, std::slice::from_ref(arg1), None)
    }
    // Resolved against the CALLING file's directory, which a compiled binary
    // still knows -- see `dynamic_require_relative`.
    module_function def "require_relative" (_recv, arg1) {
        dynamic_require_relative(arg1)
    }
    // `main.using` inside a METHOD is ruby's RuntimeError, and it is the
    // only way this row is reached: the compile-time rewrite covers a
    // lexical `using`, so a call landing here came from a method body --
    // which ruby refuses too -- or from a dynamic send, which has no
    // lexical range to rewrite. Without the row at all, the method-body
    // case answered `NoMethodError: undefined method 'using' for main`.
    //
    // A frame label starting with `<` is a genuine toplevel or class body
    // (`<main>`, `<class:K>`); anything else is a method.
    private def "using" (_recv, _module) {
        if crate::frames::current_frame_label().is_some_and(|l| !l.starts_with('<')) {
            return Err(crate::dispatch::raise_error(
                "RuntimeError",
                "main.using is permitted only at toplevel".to_string(),
            ));
        }
        Err(crate::builtins::not_impl_error!(
            "Kernel#using cannot be reached through a runtime send: zeo activates refinements at compile time"
        ))
    }
    private def "pp" params "*objs" (_recv, *args, &_block) {
        kernel_pp(args)
    }
    module_function def "warn" params "*msgs, uplevel: nil, category: nil" (_recv, *args, &_block) {
        kernel_warn(args)
    }
    // Spawning a child. `system` inherits stdout/stderr and answers a
    // true/false/nil verdict; the backtick captures stdout and answers it as a
    // String. Both set `$?` (see `builtins::process`). Private Kernel methods,
    // so `respond_to?`'s default hides them (see `is_hidden_builtin_private`).
    module_function def "system"(recv, *args, &block) {
        crate::builtins::process::system(recv, args, block)
    }
    module_function def "`"(recv, cmd, &block) {
        crate::builtins::process::backquote(recv, std::slice::from_ref(cmd), block)
    }
    // `spawn` starts the child WITHOUT waiting and answers its pid -- the
    // Kernel spelling of `Process.spawn`, which Open3's popen family calls
    // receiverless from module context.
    module_function def "spawn"(_recv, *args, &_block) {
        crate::builtins::process::spawn_pid(args)
    }
    // `putc` -- writes one character to `$stdout` and returns its argument.
    // An Integer writes the low byte (`n & 0xff`); a String writes its first
    // character.
    module_function def "putc"(_recv, arg) {
        let out = crate::builtins::io::current_stdout();
        // Shared with `IO#putc`: first character in the string's own
        // encoding (ONE raw byte for the byte encodings), an Integer's low
        // byte, NUM2CHR otherwise.
        let bytes = crate::builtins::io::putc_bytes(arg)?;
        crate::builtins::io::write_bytes(&out, &bytes)?;
        Ok((*arg).clone())
    }
    // `public_method(:name)` -- a bound Method restricted to the public
    // surface (a private/protected name raises NameError).
    def "public_method"(recv, arg) {
        crate::builtins::method::public_method_new(recv, arg)
    }
    // `Kernel#method(:name)` -- a bound Method object (see
    // `builtins::method`). Reaches every receiver via the MRO walk's
    // Kernel row, including the top-level `main` object.
    def "method"(recv, arg) {
        crate::builtins::method::method_new(recv, arg)
    }
    def "singleton_method"(recv, arg) {
        crate::builtins::method::singleton_method_new(recv, arg)
    }
    // `obj.singleton_class` -- the per-object singleton class as a real Class
    // value; defining a method on it installs a per-object singleton (see
    // `runtime_meta::runtime_singleton_class`).
    def "singleton_class"(recv) {
        crate::runtime_meta::runtime_singleton_class(recv)
    }
    // `obj.extend(Mod, ...)` -- mix each module's instance methods into the
    // receiver's singleton. The bare `extend Mod` STATEMENT form (no receiver)
    // is a separate parse-level mixin; this row is the method-call form only.
    def "extend" cfunc (recv, first, *rest, &_block) {
        // Each goes through `Module#extend_object` when the module overrides
        // it, the same route `include` takes through `append_features` -- see
        // `runtime_meta::runtime_extend`.
        //
        // LAST argument first, which is CRuby's `rb_obj_extend` loop
        // (`object.c`, `argc-1` down to `0`). Each `extend` layers ABOVE the
        // one before it, so walking backwards is what leaves the FIRST module
        // nearest the object -- `extend(A, B).singleton_class.ancestors` is
        // `[singleton, A, B, ...]`.
        let mods: Vec<&RubyValue> = std::iter::once(first).chain(rest).collect();
        for m in mods.into_iter().rev() {
            crate::runtime_meta::runtime_extend(recv, m)?;
        }
        Ok(recv.clone())
    }
    // `Object#define_singleton_method(name) { body }` -- a per-object
    // singleton on an ordinary receiver, or a class/singleton method when the
    // receiver is a `Class`. Universal (this Kernel row is reached by every
    // receiver's MRO walk, including a class value). A singleton on an
    // immediate (Integer/Symbol/nil/...) is a `TypeError`, like CRuby.
    def "define_singleton_method" cfunc (recv, name, body?, &block) {
        let name = crate::runtime_meta::coerce_method_name(Some(name))?;
        if let Some(src) = body
            && let Some((owner, src_name)) = crate::builtins::method::method_source(src) {
                return crate::runtime_meta::runtime_define_singleton_from_method(
                    recv, name, owner, src_name);
            }
        let body = crate::runtime_meta::coerce_method_body(body, &block)?;
        crate::runtime_define_singleton_method(recv, name, body)
    }
    // `eval(str)` -- runtime string eval, which zeo COMPILES (a binary
    // linked without the compiler answers NotImplementedError).
    // `self` is the CALLER's own, since this universal Kernel row is reached
    // through the receiver's MRO walk -- so `eval("@x")` at the top level reads
    // the main object's ivar, and the same call inside a method reads that
    // receiver's. An explicit `Binding` argument instead runs the source in
    // THAT captured scope (its locals, its `self`, its cref); `nil` means the
    // current context, as in CRuby. The filename/lineno arguments set what
    // `__FILE__`/`__LINE__` report inside the source.
    module_function def "eval" cfunc (recv, arg1, arg2?, arg3?, arg4?) {
        let file = match arg3 {
            Some(v) if !v.is_nil() => {
                Some(crate::builtins::convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned())
            }
            _ => None,
        };
        let line = match arg4 {
            Some(v) if !v.is_nil() => Some(crate::builtins::convert::to_index(v)? as u32),
            _ => None,
        };
        match arg2 {
            None | Some(RubyValue::Nil) => crate::eval::eval_value_located(
                (*arg1).clone(),
                recv.clone(),
                0,
                crate::eval::EvalMode::Caller,
                file.as_deref(),
                line,
            ),
            Some(b) => {
                let Some(b) = crate::builtins::binding::as_binding(b) else {
                    return Err(crate::builtins::wrong_arg_type(b, "binding"));
                };
                crate::eval::eval_with_binding(arg1, b, file, line, "Kernel#eval")
            }
        }
    }
    // `catch(tag = new object) { |tag| ... }` / `throw(tag[, value])` /
    // `sleep(secs)` -- universal Kernel methods. The static codegen fast path
    // handles the literal `catch {}`/`throw` forms; these rows serve dynamic
    // dispatch (a `send :catch`, a `catch` reached through the MRO walk).
    module_function def "catch"(_recv, tag?, &block) {
        // A bare `catch` mints a fresh, unique tag object (passed to the block).
        // A plain `Object`, as `rb_catch` does -- the block is handed the tag
        // and `t.class` must answer `Object`. Uniqueness comes from the
        // allocation, since the tag is matched by identity.
        let tag = tag
            .cloned()
            .unwrap_or_else(|| crate::runtime_meta::blank_instance(zeo_abi::OBJECT_CLASS));
        let blk = block.ok_or_else(|| {
            local_jump_error!("no block given (yield)")
        })?;
        crate::kernel_catch(tag, blk)
    }
    module_function def "throw" as kernel_throw cfunc (_recv, _tag, _value?) {
        crate::catch::throw_impl(__args)
    }
    // `Kernel#raise`/`#fail` as REAL dispatch rows -- reached by
    // `send(:raise, ...)` and by builtin-alias rewrites (`alias_method
    // :raise!, :raise`). Statically-written `raise` never comes here (parse
    // lowers it to `HirNode::Raise`); this is the dynamic mirror of
    // the emitter's raise lowering, following CRuby's `rb_make_exception`: the
    // operand's own `exception` method constructs the value (running a user
    // subclass's `initialize`), a String implies `RuntimeError`, an
    // exception OBJECT with no message raises as-is, anything else is
    // `TypeError: exception class/object expected`. A third argument is a
    // CUSTOM backtrace and replaces the stamped one; an explicit `cause:`
    // doesn't reach this row (kwargs ride as a trailing Hash that 2-arg
    // shapes would misread; the automatic `$!` chaining below is what
    // dynamic callers get).
    module_function def "raise" | "fail"(_recv, exception?, message?, backtrace?) {
        let args: Vec<RubyValue> = [exception, message]
            .iter()
            .take_while(|p| p.is_some())
            .filter_map(|p| p.cloned())
            .collect();
        let exc = build_raise_exception(&args)?;
        let exc = crate::dispatch::raise_with_cause(exc);
        // The innermost frame's label names the method this raise is IN, and
        // that name has no call on the raise's own line. The verb does.
        crate::builtins::exception::mark_explicitly_raised(&exc, "raise");
        // After the stamp, so the custom lines WIN over the real stack.
        if let Some(bt) = backtrace {
            crate::builtins::exception::apply_custom_backtrace(&exc, bt)?;
        }
        Err(Signal::Raise(exc))
    }
    module_function def "sleep" as kernel_sleep (_recv, _seconds?) {
        sleep_impl(__args)
    }
    def "class"(recv) {
        Ok(RubyValue::Class(recv.class_id()))
    }
    // `object_id`. Its `__id__` twin is BasicObject's, which is where CRuby
    // owns it, so both rows share `object_id_of`.
    def "object_id"(recv) {
        Ok(object_id_of(recv))
    }
    def "nil?" (recv) {
        Ok(RubyValue::Bool(recv.is_nil()))
    }
    def "itself"(recv) {
        Ok(recv.clone())
    }
    module_function def "caller"(_recv, start?, length?) {
        // The formatted frames above the calling frame (this builtin has no
        // frame of its own, so `start = 1` -- the default -- skips exactly
        // the caller). `caller(0)` includes the caller itself; a `start`
        // past the top answers nil (not [] -- oracle-verified); an optional
        // `length` truncates. The Range form is served by the same window.
        let all = crate::frames::caller_lines(0);
        let (start, length) = caller_window(start, length);
        if start > all.len() {
            return Ok(RubyValue::Nil);
        }
        let mut window: Vec<RubyValue> = all[start..]
            .iter()
            .map(|l| RubyValue::Str(crate::string_new(l.clone())))
            .collect();
        if let Some(l) = length {
            window.truncate(l);
        }
        Ok(RubyValue::Array(crate::array_new(window)))
    }
    // `caller`'s object form: the same window over the same frames, each entry
    // a `Thread::Backtrace::Location` with real `#path`/`#lineno`/`#label`
    // (forwardable builds its deprecation message out of them).
    module_function def "caller_locations"(_recv, start?, length?) {
        let all = crate::frames::caller_frames(0);
        let (start, length) = caller_window(start, length);
        if start > all.len() {
            return Ok(RubyValue::Nil);
        }
        // Callees come off the WHOLE list before the window is cut: a
        // windowed location 0 still calls into the frame ahead of it, and
        // only the full list names that. The full list's own location 0 is
        // the frame that called `caller_locations`, so what it invoked is
        // this row.
        let rows: Vec<(String, u32, String)> = all
            .iter()
            .map(|(f, l, m)| ((*f).to_string(), *l, (*m).to_string()))
            .collect();
        let mut window = crate::builtins::backtrace_location::thread_callees(
            &rows,
            Some("caller_locations".to_string()),
        );
        window.drain(..start);
        if let Some(l) = length {
            window.truncate(l);
        }
        Ok(RubyValue::Array(crate::array_new(window)))
    }
    // The private `Kernel` conversion and formatting functions, as real methods
    // so they resolve through EVERY dispatch path -- a splat call (`format(*a)`),
    // `method(:Integer)`, `send`, a curry -- not only the codegen fast-path that
    // intercepts a direct literal call. The `as` name IS what codegen emits
    // (`zeo_rt::kernel_integer`), so the fast path and the dispatch row are one
    // function and cannot disagree about the argument count.
    module_function def "format" | "sprintf"(_recv, *args, &_block) {
        kernel_format(args)
    }
    // NO `**opts`: the DSL peels any trailing hash before the arity guard, so
    // `Integer({})` lost its only argument and failed the count. The
    // `exception:` keyword is taken by `with_exception_kw` instead, which peels
    // a trailing hash only when it really carries that key -- leaving an
    // ordinary Hash argument positional, where it belongs.
    module_function def "Integer" params "arg, base = 0, exception: true" as kernel_integer (_recv, _arg, _base?, _opts?) {
        let _frame = conversion_frame("Kernel#Integer");
        with_exception_kw(__args, integer_impl)
    }
    // The second slot is the `exception:` hash -- see `Integer`'s note.
    module_function def "Float" params "arg, exception: true" as kernel_float (_recv, _arg, _opts?) {
        let _frame = conversion_frame("Kernel#Float");
        with_exception_kw(__args, float_impl)
    }
    module_function def "String" as kernel_string (_recv, _arg) {
        let _frame = conversion_frame("Kernel#String");
        string_impl(__args)
    }
    module_function def "Array" as kernel_array (_recv, _arg) {
        let _frame = conversion_frame("Kernel#Array");
        array_impl(__args)
    }
    module_function def "Hash" as kernel_hash (_recv, _arg) {
        let _frame = conversion_frame("Kernel#Hash");
        hash_impl(__args)
    }
    module_function def "Rational" as kernel_rational cfunc (_recv, _numerator, _denominator?, _opts?) {
        let _frame = conversion_frame("Kernel#Rational");
        with_exception_kw(__args, rational_impl)
    }
    // `Kernel#BigDecimal` -- the one BigDecimal constructor (`.new` is long
    // removed). Present whenever the extension is compiled in; like `Time`'s
    // extra methods, it answers even without `require "bigdecimal"`.
    #[cfg(feature = "ext-bigdecimal")]
    private def "BigDecimal" cfunc (_recv, _initial, _digits?) {
        crate::ext::bigdecimal::kernel_big_decimal(__args)
    }
    // `Kernel#Pathname(str)` -- PRIVATE, and there with no require, because
    // ruby 4.0 loads `pathname.so` before the first line.
    module_function def "Pathname" params "path" (_recv, arg) {
        crate::builtins::pathname::kernel_pathname(arg)
    }
    module_function def "Complex" as kernel_complex cfunc (_recv, _real, _imaginary?, _opts?) {
        let _frame = conversion_frame("Kernel#Complex");
        with_exception_kw(__args, complex_impl)
    }
    // `Kernel#autoload`/`#autoload?` -- the RECEIVERLESS spellings, which
    // register on `Object` rather than on the caller's class. `Module`'s rows
    // hold the implementation; only the receiver differs.
    //
    // ruby reads the caller's cref here, so `class Foo; autoload :X, "y"; end`
    // lands on `Foo`. zeo folds that literal form at compile time
    // (`parse::loader` splices the feature), so what reaches this row is the
    // computed residue -- `send(:autoload, ...)` -- where no cref is knowable
    // and `Object` is what ruby itself uses at top level.
    module_function def "autoload"(_recv, _sym, _path) {
        inherited_row!(rmodule, "autoload", &object_value(), __args, None)
    }
    module_function def "autoload?" cfunc (_recv, _sym, _inherit?) {
        inherited_row!(rmodule, "autoload?", &object_value(), __args, None)
    }
    // `rand`/`srand` as real rows: without them the argument-count guard would
    // have to live in the runtime routine, where the codegen fast path is the
    // only caller that reaches it -- the double declaration this whole DSL
    // removes. They answer through dispatch now too (`send(:rand)`), which they
    // did not before.
    module_function def "rand" as kernel_rand (_recv, _max?) {
        rand_impl(__args)
    }
    module_function def "srand" as kernel_srand (_recv, _seed?) {
        srand_impl(__args)
    }
    // Private `Kernel#trap` -- the receiverless spelling of `Signal.trap`, same
    // validated no-op that records the action and returns the prior one.
    module_function def "trap" cfunc (_recv, sig, command?, &block) {
        crate::builtins::signal::trap_impl(sig, command, block)
    }
    // `proc(&b)` / `proc { }` -- answer the passed block as a Proc (it already IS
    // one at the ABI level). No block is CRuby's `ArgumentError`.
    module_function def "proc"(_recv, &block) {
        match block {
            Some(b @ RubyValue::Proc(_)) => Ok(b),
            _ => Err(arg_error!("tried to create Proc object without a block")),
        }
    }
    def "dup"(recv) {
        Ok(match recv {
            RubyValue::Object(o) => {
                let c = copy_via_hook(recv, RubyValue::Object(o.dup_object(false)), "initialize_dup", None)?;
                crate::builtins::rstruct::refreeze_data_copy(&c);
                c
            }
            // A MODULE gets a real copy -- the whole point of duping one is to
            // edit it without touching the original (see `runtime_module_dup`).
            RubyValue::Class(cid) if crate::dispatch::class_is_module(*cid).unwrap_or(false) => {
                crate::runtime_meta::runtime_module_dup(*cid)?
            }
            // ...and so does a CLASS: an ANONYMOUS class sharing the source's
            // superclass and mixins, carrying copies of its own rows,
            // constants and class-level state. Handing back the same handle
            // made `K.dup.equal?(K)` true, so naming the copy renamed `K`.
            RubyValue::Class(cid) => crate::runtime_meta::runtime_class_dup(*cid, false)?,
            _ => recv.dup_value(false)?,
        })
    }
    def "clone" params "freeze: nil" (recv, arg?) {
        // `clone(freeze: nil)` PRESERVES the original's frozen state (the
        // default), `freeze: true` forces the copy frozen, `freeze: false`
        // forces it unfrozen. The keyword arrives as a trailing options Hash.
        let freeze = match arg {
            Some(RubyValue::Hash(h)) => {
                match crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("freeze"))) {
                    RubyValue::Bool(b) => Some(b),
                    _ => None,
                }
            }
            _ => None,
        };
        // An immediate (nil/true/false/Integer/Float/Symbol) is permanently
        // frozen -- `clone(freeze: false)` can't unfreeze it, so CRuby raises
        // rather than handing back a mutable copy.
        if freeze == Some(false)
            && matches!(
                recv,
                RubyValue::Nil
                    | RubyValue::Bool(_)
                    | RubyValue::Int(_)
                    | RubyValue::BigInt(_)
                    | RubyValue::Float(_)
                    | RubyValue::Symbol(_)
            )
        {
            return Err(arg_error!("can't unfreeze {}", crate::builtins::class_name_of(recv)));
        }
        let copy_frozen = freeze != Some(false);
        let copy = match recv {
            RubyValue::Object(o) => {
                // CRuby's order: the copy exists UNFROZEN while the copy
                // hooks run and the frozen bit lands after -- a hook that
                // refuses frozen receivers (Data's) sees the pre-freeze copy.
                let c = copy_via_hook(
                    recv,
                    RubyValue::Object(o.dup_object(false)),
                    "initialize_clone",
                    freeze,
                )?;
                if copy_frozen && recv.is_frozen() {
                    let _ = c.freeze_value();
                }
                // Data copies stay frozen even under `freeze: false` (#2716).
                crate::builtins::rstruct::refreeze_data_copy(&c);
                c
            }
            // The class/module half, as `dup` above -- `clone` additionally
            // carries the source's frozen state.
            RubyValue::Class(cid) if crate::dispatch::class_is_module(*cid).unwrap_or(false) => {
                crate::runtime_meta::runtime_module_dup(*cid)?
            }
            RubyValue::Class(cid) => {
                crate::runtime_meta::runtime_class_dup(*cid, copy_frozen)?
            }
            _ => recv.dup_value(copy_frozen)?,
        };
        // `clone` carries the singleton class -- its methods and extended
        // modules -- where `dup` drops it (Ruby's rule).
        crate::runtime_meta::copy_value_singletons(recv, &copy);
        if freeze == Some(true) {
            copy.freeze_value()?;
        }
        Ok(copy)
    }
    def "frozen?"(recv) {
        Ok(RubyValue::Bool(recv.is_frozen()))
    }
    def "freeze"(recv) {
        recv.freeze_value()
    }
    def "hash"(recv) {
        Ok(RubyValue::Int(crate::value_hash_code(recv)))
    }
    def "to_s" (recv) {
        // Fallible: `[obj].to_s` re-enters a user `inspect` per element,
        // and a raising one propagates (catchable, CRuby's rule).
        Ok(RubyValue::Str(crate::string_new(crate::value::default_to_s(recv)?)))
    }
    def "inspect" (recv) {
        Ok(RubyValue::Str(crate::string_new(crate::value::default_inspect(recv)?)))
    }
    // Kernel's default `===` is `==` (case subjects fall back to equality).
    def "==="(recv, other) {
        Ok(RubyValue::Bool(recv.rb_eq(other)))
    }
    // The default `Object#<=>`: `0` when the two are `==`, else `nil` (no
    // ordering). Classes with a real ordering (Integer/String/Array/... )
    // define their own `<=>`, which the MRO walk reaches before this Kernel
    // fallback, so this only answers for the un-ordered types (Hash, Range,
    // Regexp, nil, true/false, Proc, Complex).
    def "<=>"(recv, other) {
        Ok(if recv.rb_eq(other) {
            RubyValue::Int(0)
        } else {
            RubyValue::Nil
        })
    }
    // `Object#eql?` is IDENTITY in CRuby (`rb_obj_equal`); every type that
    // wants value semantics owns its own row. zeo's Object-backed handle
    // kinds (a Random, a File::Stat, a Dir, a Method) inherit this one and
    // must get identity, or `Random.new(7).eql?(Random.new(7))` answers
    // true where ruby says false.
    //
    // Every OTHER `RubyValue` variant reaching this row is a kind CRuby
    // owns `eql?` on, and zeo's rows for those delegate here rather than
    // repeating the comparison -- so they keep the value form: same class
    // AND `==`, which is what makes `1.eql?(1.0)` false while `1 == 1.0`
    // is true.
    def "eql?"(recv, arg) {
        if matches!(recv, RubyValue::Object(_)) && !owns_value_eql(recv) {
            return Ok(RubyValue::Bool(crate::builtins::basic_object::value_identity(recv, arg)));
        }
        Ok(RubyValue::Bool(
            recv.class_id() == (*arg).class_id() && recv.rb_eq(arg),
        ))
    }
    def "is_a?" | "kind_of?"(recv, arg) {
        let RubyValue::Class(target) = arg else {
            return Err(type_error!("class or module required"));
        };
        Ok(RubyValue::Bool(crate::dispatch::is_a_value(recv, *target)))
    }
    def "instance_of?"(recv, arg) {
        let RubyValue::Class(target) = arg else {
            return Err(type_error!("class or module required"));
        };
        Ok(RubyValue::Bool(recv.class_id() == *target))
    }
    // `respond_to?(name, include_all = false)` -- the second parameter
    // opts private methods back in (CRuby's default ignores them).
    def "respond_to?" cfunc (recv, arg1, arg2?) {
        let sym = match arg1 {
            RubyValue::Symbol(s) => *s,
            RubyValue::Str(s) => Symbol::intern(&s.lock().to_utf8_lossy()),
            other => {
                return Err(type_error!("{} is not a symbol nor a string", other.inspect_string()))
            }
        };
        let include_all = arg2.is_some_and(|v| v.truthy());
        // The full protocol (`responds_to_or_missing`): per-object
        // singletons, the value-aware walk, then the receiver's
        // `respond_to_missing?` hook on a miss.
        Ok(RubyValue::Bool(crate::dispatch::responds_to_or_missing(
            recv, sym, include_all)?))
    }
    // The copy hooks, all three PRIVATE and owned by `Kernel` -- not by
    // `Object`, where a top-level `def initialize_copy` in the prelude used to
    // put the first one (which also made it the only name in
    // `Object.private_instance_methods(false)`, where ruby answers `[]`).
    //
    // Each is a no-op that answers the receiver: the runtime `clone`/`dup` has
    // already made the shallow ivar copy by the time the user-overridable hook
    // runs. `initialize_dup`/`initialize_clone` are what CRuby's `dup`/`clone`
    // call, and both reach `initialize_copy` from there, so a user override of
    // the one hook still sees both paths.
    private def "initialize_copy"(recv, _orig) {
        Ok(recv.clone())
    }
    // Both DISPATCH `initialize_copy` rather than calling Kernel's row: a
    // class that overrides `initialize_copy` must still see it when `dup`
    // reaches this default through `initialize_dup`'s `super`.
    private def "initialize_dup"(recv, orig) {
        crate::dispatch::send_value(
            recv, crate::Symbol::intern("initialize_copy"), std::slice::from_ref(orig), None)
    }
    private def "initialize_clone" cfunc (recv, orig, *_opts) {
        crate::dispatch::send_value(
            recv, crate::Symbol::intern("initialize_copy"), std::slice::from_ref(orig), None)
    }
    // `Kernel#instance_variables_to_inspect` default: nil, meaning "show
    // every ivar". An override returning an Array turns `#inspect`'s ivar
    // list into a members-only filter -- see `value::default_object_repr`.
    private def "instance_variables_to_inspect"(_recv) {
        Ok(RubyValue::Nil)
    }

    // `Object#respond_to_missing?` default: false for every name -- what a
    // user override's `super` reaches (CRuby's
    // `rb_obj_respond_to_missing`). Hidden-private, like `initialize`.
    private def "respond_to_missing?"(_recv, _name, _include_private) {
        Ok(RubyValue::Bool(false))
    }
    // Universal named-ivar reflection over ANY receiver (an `Object`'s or a
    // class object's ivars; a builtin/immediate exposes none). Registering
    // these on Kernel is also what makes `respond_to?(:instance_variable_get)`
    // and a dynamic `send(:instance_variables)` resolve them uniformly -- the
    // static codegen path (call.rs) is just a fast path over the same helpers.
    def "instance_variables"(recv) {
        Ok(crate::dispatch::instance_variables(recv))
    }
    def "instance_variable_get"(recv, arg) {
        crate::dispatch::instance_variable_get(recv, arg)
    }
    def "instance_variable_set"(recv, arg1, arg2) {
        crate::dispatch::instance_variable_set(recv, arg1, (*arg2).clone())
    }
    def "remove_instance_variable"(recv, arg) {
        crate::dispatch::remove_instance_variable(recv, arg)
    }
    def "instance_variable_defined?"(recv, arg) {
        let name = crate::dispatch::ivar_name_arg(arg)?;
        let sym = Symbol::intern(&format!("@{name}"));
        let RubyValue::Array(vars) = crate::dispatch::instance_variables(recv) else {
            unreachable!("instance_variables always answers an Array")
        };
        let found = vars
            .lock()
            .iter()
            .any(|v| matches!(v, RubyValue::Symbol(s) if *s == sym));
        Ok(RubyValue::Bool(found))
    }
    // `obj.methods` -- public+protected names callable on the receiver: its
    // class's instance methods across the ancestry, plus (for a class/module
    // receiver) that class's own `def self.` methods. A builtin's list is a
    // subset of CRuby's (this runtime implements a subset), so callers assert
    // membership; a plain user object's list is exact.
    def "methods"(recv, arg?) {
        // `methods(false)` IS `singleton_methods(false)` in CRuby: the class's
        // instance methods drop out entirely rather than narrowing to the own
        // list, which is why this cannot share `public_methods`' body.
        if !truthy_arg(arg) {
            return Ok(syms_to_array(own_singleton_names(recv)));
        }
        let mut names = Vec::new();
        if let RubyValue::Class(cid) = recv {
            names.extend(crate::dispatch::public_class_method_names(*cid, true));
        }
        // `Object#methods` returns public AND protected names -- the object's
        // OWN rows included, which no class-id walk can reach.
        names.extend(singleton_names_where(
            recv, |v| v != crate::dispatch::MethodVisibility::Private));
        names.extend(crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::VisFilter::NotPrivate,
            true,
        ));
        Ok(syms_to_array(dedup_syms(names)))
    }
    // A per-object singleton row sits AHEAD of the class chain, so all three
    // report it. Its visibility is the object's own mark, not anything the
    // class walk can answer.
    def "public_methods"(recv, arg?) {
        let inherit = truthy_arg(arg);
        let mut names = singleton_names_where(
            recv, |v| v == crate::dispatch::MethodVisibility::Public);
        if let RubyValue::Class(cid) = recv {
            names.extend(crate::dispatch::public_class_method_names(*cid, inherit));
        }
        names.extend(crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::VisFilter::NotPrivate,
            inherit,
        ));
        Ok(syms_to_array(dedup_syms(names)))
    }
    def "private_methods"(recv, arg?) {
        let inherit = truthy_arg(arg);
        let mut names = singleton_names_where(
            recv, |v| v == crate::dispatch::MethodVisibility::Private);
        // `main`'s private singletons are routed rather than installed, so
        // no table holds them -- see `dispatch::MAIN_PRIVATE_SINGLETONS`.
        if matches!(recv, RubyValue::Object(o) if crate::dispatch::is_main_object(o)) {
            names.extend(
                crate::dispatch::MAIN_PRIVATE_SINGLETONS
                    .iter()
                    .map(|n| Symbol::intern(n)),
            );
        }
        names.extend(crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::VisFilter::Private,
            inherit,
        ));
        Ok(syms_to_array(dedup_syms(names)))
    }
    def "protected_methods"(recv, arg?) {
        let inherit = truthy_arg(arg);
        let mut names = singleton_names_where(
            recv, |v| v == crate::dispatch::MethodVisibility::Protected);
        names.extend(crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::VisFilter::Protected,
            inherit,
        ));
        Ok(syms_to_array(dedup_syms(names)))
    }
    // A class/module receiver's own singleton methods are its `def self.`
    // methods; other receivers have no per-object singletons in this runtime's
    // value model, so they report an empty list.
    def "singleton_methods"(recv, arg?) {
        // A class's singleton methods are its class methods; any other
        // receiver's are the ones installed on it BY IDENTITY at runtime.
        let names = match (recv, truthy_arg(arg)) {
            (RubyValue::Class(cid), inherit) => {
                crate::dispatch::public_class_method_names(*cid, inherit)
            }
            _ => own_singleton_names(recv),
        };
        Ok(syms_to_array(names))
    }
    // `Object#display([port])` -- writes `self.to_s` (no newline) to stdout
    // and answers nil. The optional port argument is accepted but ignored
    // (only the process stdout is modeled).
    def "display"(recv, _arg?) {
        kernel_print(std::slice::from_ref(recv))
    }
    // `Object#!~` -- the negation of `=~`, dispatched to the receiver's own
    // `=~` (so a receiver without one raises NoMethodError, exactly as CRuby
    // does since `Object#=~` was removed).
    def "!~"(recv, other) {
        let matched = crate::dispatch::send_value(
            recv, crate::Symbol::intern("=~"), std::slice::from_ref(other), None)?;
        Ok(RubyValue::Bool(!matched.truthy()))
    }
    def "tap"(recv, &block) {
        let p = need_block!(block);
        p.call(std::slice::from_ref(recv))?;
        Ok(recv.clone())
    }
    def "then" | "yield_self"(recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        p.call(std::slice::from_ref(recv))
    }
    // `Kernel#loop` -- blockless is an infinite Enumerator; with a block it
    // loops forever, rescuing StopIteration and returning its `#result`
    // (a literal `loop do…end` is desugared in the lowerer, so this handles
    // the blockless and block-pass forms + the Enumerator re-invoke).
    module_function def "loop"(recv, &block) {
        let Some(RubyValue::Proc(p)) = &block else {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "loop", &[]));
        };
        loop {
            match p.call(&[]) {
                Ok(_) => {}
                Err(Signal::Break(v)) => return Ok(v),
                Err(Signal::Raise(exc))
                    if crate::dispatch::is_a(exc.class_id(), zeo_abi::STOP_ITERATION_CLASS) =>
                {
                    return crate::dispatch::send_value(
                        &exc,
                        Symbol::intern("result"),
                        &[],
                        None,
                    );
                }
                Err(sig) => return Err(sig),
            }
        }
    }
    // `x.to_enum(:meth, *args)` -- captures exactly (receiver, method,
    // args), CRuby's obj_to_enum. An optional block SUPPLIES the size, and is
    // stored unevaluated: `def each(&b) = block_given? ? ... : to_enum(:each)
    // { @items.size }` is the documented way to write `each`, and the block
    // exists so the count is computed only if someone asks for it.
    def "to_enum" | "enum_for"(recv, *args, &block) {
        let meth = match args.first() {
            None => "each".to_string(),
            Some(RubyValue::Symbol(s)) => s.name().as_str().to_string(),
            Some(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
            Some(other) => {
                return Err(type_error!("{} is not a symbol nor a string", other.inspect_string()))
            }
        };
        let rest = if args.is_empty() { &[] } else { &args[1..] };
        let size = block.filter(|b| matches!(b, RubyValue::Proc(_)));
        Ok(crate::builtins::enumerator::enumerator_for_with_size(
            recv, &meth, rest, size,
        ))
    }
    // The line-input family. CRuby reads these from ARGF, which -- with no
    // file arguments -- IS `$stdin`. zeo has no ARGF, so they forward to
    // `$stdin` directly and answer identically for every script that is not
    // a `while gets` filter over `ARGV`.
    module_function def "gets"(_recv, *args, &_block) {
        stdin_send("gets", args)
    }
    module_function def "readline"(_recv, *args, &_block) {
        stdin_send("readline", args)
    }
    module_function def "readlines"(_recv, *args, &_block) {
        stdin_send("readlines", args)
    }
    // `select` and `exec` are the same calls as `IO.select` and
    // `Process.exec`, which is exactly how CRuby defines them.
    module_function def "select"(_recv, *args, &_block) {
        crate::dispatch::send_value(
            &RubyValue::Class(zeo_abi::IO_CLASS),
            Symbol::intern("select"),
            args,
            None,
        )
    }
    module_function def "exec"(_recv, *args, &_block) {
        crate::dispatch::send_value(
            &RubyValue::Class(zeo_abi::PROCESS_CLASS),
            Symbol::intern("exec"),
            args,
            None,
        )
    }
    // `test(?e, path)` -- the one-character file tests, each the `File`
    // predicate of the same meaning. The two-file comparison commands
    // (`?=`, `?<`, `?>`, `?-`) are not served here.
    module_function def "test" cfunc (_recv, arg1, arg2) {
        kernel_test(arg1, arg2)
    }
    // `trace_var(:$g) { |v| }` / `trace_var(:$g, command)` -- run something on
    // every assignment RUBY makes to a global. The runtime's own seeding is not
    // an assignment and fires nothing.
    module_function def "trace_var" cfunc (_recv, arg1, arg2?, &block) {
        let name = global_name_arg(arg1)?;
        let command = match (arg2, block) {
            (Some(c), _) if !matches!(c, RubyValue::Nil) => c.clone(),
            (_, Some(b)) => b,
            // CRuby reaches this through `Proc.new`, and reports it that way.
            _ => return Err(arg_error!("tried to create Proc object without a block")),
        };
        crate::globals::trace_var(&name, command);
        Ok(RubyValue::Nil)
    }
    // `untrace_var(:$g)` drops every hook, `untrace_var(:$g, command)` just the
    // one, and both answer what they dropped. A name that was never assigned
    // AND never traced is a NameError.
    module_function def "untrace_var" cfunc (_recv, arg1, arg2?) {
        let name = global_name_arg(arg1)?;
        if !crate::globals::is_traced(&name) && !crate::globals::global_defined(0, &name) {
            return Err(crate::builtins::name_error!("undefined global variable {name}"));
        }
        let dropped = crate::globals::untrace_var(&name, arg2);
        Ok(RubyValue::Array(crate::array_new(dropped)))
    }
    // Every global with a value in THIS box, plus the specials, which read
    // from the runtime rather than the store and so are never in it. CRuby
    // reports its full predefined set in an unspecified order; zeo reports
    // the ones it models, which is what an `include?` probe asks about.
    module_function def "global_variables"(_recv) {
        let mut names: Vec<String> = crate::globals::defined_globals();
        names.sort();
        Ok(RubyValue::Array(crate::array_new(
            names.into_iter().map(|n| RubyValue::Symbol(Symbol::intern(n.as_str()))).collect(),
        )))
    }

    // ---- the once-folded intrinsics, as real rows.
    //
    // Every spelling reaches these rows: a direct `printf(...)`, a
    // `Kernel.printf(...)` (`lower` drops the redundant receiver), and the
    // reflective forms. Without a table row, `send(:printf, ...)` raised
    // NoMethodError, `method(:printf)` raised NameError and
    // `respond_to?(:printf, true)` answered false -- ruby answers all three.
    // `module_function`, which is how ruby has them: a PRIVATE
    // instance copy (`send(:printf, ...)`) and a PUBLIC singleton one
    // (`Kernel.printf(...)`).
    module_function def "printf" cfunc (_recv, *_args) { kernel_printf(__args) }
    module_function def "exit" cfunc (_recv, *_args) { Err(kernel_exit(__args)) }
    module_function def "abort" cfunc (_recv, *_args) { Err(kernel_abort(__args)) }
    module_function def "exit!" cfunc (_recv, *_args) { kernel_exit_bang(__args) }
    module_function def "at_exit"(_recv, &block) {
        let handler = crate::builtins::need_block!(block);
        let handler = RubyValue::Proc(handler);
        crate::exec::at_exit_register(handler.clone());
        Ok(handler)
    }
    // `lambda { }` answers the block as a LAMBDA -- `#lambda?` true, and a
    // `return` inside it returns from the lambda rather than its defining
    // method.
    //
    // It takes a block WRITTEN AT THE SITE THAT INVOKES IT and refuses one
    // that arrives any other way. The rule is a property of the block
    // HANDLER, not of the block: `RProc::is_literal_block` carries it.
    module_function def "lambda"(_recv, &block) {
        let Some(RubyValue::Proc(p)) = block else {
            return Err(crate::builtins::arg_error!(
                "tried to create Proc object without a block"
            ));
        };
        // An iseq handler becomes a lambda. A proc handler that IS ALREADY a
        // lambda comes back UNCHANGED -- same object, which
        // `lambda(&lam).equal?(lam)` reads. Anything else is refused, and
        // `Symbol#to_proc`/`Method#to_proc` pass because both are lambdas.
        if p.is_lambda() {
            return Ok(RubyValue::Proc(p));
        }
        if !p.is_literal_block() {
            return Err(crate::builtins::arg_error!(
                "the lambda method requires a literal block"
            ));
        }
        Ok(RubyValue::Proc(p.as_lambda()))
    }
    // The frame readers. A builtin row pushes no frame of its own, so the
    // CURRENT frame is the caller's -- the same reason `#caller` defaults to
    // `start = 1`.
    module_function def "__method__" | "__callee__"(_recv) {
        Ok(match crate::frames::current_frame_method() {
            Some(name) => RubyValue::Symbol(Symbol::intern(name)),
            None => RubyValue::Nil,
        })
    }
    module_function def "__dir__"(_recv) {
        let Some((file, _)) = crate::frames::current_location() else {
            return Ok(RubyValue::Nil);
        };
        let abs = crate::builtins::file::expand_path_of(file, None)?;
        Ok(RubyValue::Str(crate::string_new(
            crate::builtins::file::dirname_of(&abs),
        )))
    }
    // Refused, and refused the way ruby refuses them on a machine without
    // the call -- a NotImplementedError naming the function, not a
    // NoMethodError naming the method.
    // `Kernel#fork` IS `Process.fork` in ruby -- same primitive, two names --
    // so it dispatches there rather than forking itself, which also keeps a
    // gem's `Process._fork` hook in the path.
    module_function def "fork"(_recv, &block) {
        crate::dispatch::send_value(
            &RubyValue::Class(zeo_abi::PROCESS_CLASS),
            Symbol::intern("fork"),
            &[],
            block,
        )
    }
    // `nil` clears a hook that was never installed, so it succeeds; a Proc
    // would be accepted and then never called, so it is refused instead --
    // the rule `Thread#set_trace_func` and `TracePoint.new` already follow.
    module_function def "set_trace_func"(_recv, arg) {
        match arg {
            RubyValue::Nil => {
                #[cfg(feature = "ext-tracepoint")]
                crate::ext::tracepoint::set_legacy_hook(None);
                Ok(RubyValue::Nil)
            }
            #[cfg(feature = "ext-tracepoint")]
            RubyValue::Proc(p) => {
                crate::ext::tracepoint::set_legacy_hook(Some(p.clone()));
                Ok(arg.clone())
            }
            #[cfg(not(feature = "ext-tracepoint"))]
            RubyValue::Proc(_) => Err(crate::builtins::not_impl_error!("set_trace_func is not supported: this build carries no trace hooks")),
            _ => Err(type_error!("trace_func needs to be Proc")),
        }
    }
    module_function def "syscall" arity 0 (_recv, *_args) {
        Err(crate::builtins::not_impl_error!("syscall() function is unimplemented on this machine"))
    }

    // The caller-scope intrinsics. Every direct or literal-`send` spelling
    // folds into the caller at compile time (`lower`'s rewrite and the
    // emitter's intrinsic lowering), so these rows exist for REFLECTION --
    // `respond_to?(:block_given?, true)`, `method(:binding)`, the private
    // NoMethodError for an explicit receiver -- and for the one spelling no
    // fold can serve: a `send` whose name is computed at runtime. A row
    // cannot see its caller's block or scope, so that spelling is refused
    // loudly rather than answered wrongly (the `set_trace_func` rule; see
    // tests/gaps/kernel_scope_intrinsics_dynamic_send.rb).
    module_function def "block_given?" | "iterator?" (_recv) {
        Err(crate::builtins::not_impl_error!("block_given? cannot be reached through a runtime-computed send: a method row cannot see the caller's block"))
    }
    module_function def "binding"(_recv) {
        Err(crate::builtins::not_impl_error!("binding cannot be reached through a runtime-computed send: a method row cannot see the caller's scope"))
    }
    module_function def "local_variables"(_recv) {
        Err(crate::builtins::not_impl_error!("local_variables cannot be reached through a runtime-computed send: a method row cannot see the caller's scope"))
    }
}

/// A global's name as `trace_var`/`untrace_var` take it: a Symbol or a String,
/// used verbatim. CRuby validates nothing here -- `trace_var(:notaglobal)` is
/// accepted and simply never fires.
fn global_name_arg(v: &RubyValue) -> Result<String, Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(s.name().as_str().to_string()),
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            crate::dispatch::describe_receiver(other)
        )),
    }
}

/// One `Kernel#test` command, as the `File` predicate that means the same
/// thing. `?s` and the three time commands answer a value rather than a
/// boolean, and `File` already returns exactly what CRuby's `test` does for
/// each (`nil` for an empty file's size, a Time for the stamps).
fn kernel_test(cmd: &RubyValue, path: &RubyValue) -> Result<RubyValue, Signal> {
    let byte = match cmd {
        RubyValue::Int(n) => u8::try_from(*n).ok(),
        RubyValue::Str(s) => s.lock().bytes().first().copied(),
        _ => None,
    };
    let file_method = match byte {
        Some(b'e') => "exist?",
        Some(b'f') => "file?",
        Some(b'd') => "directory?",
        Some(b'l') => "symlink?",
        Some(b'p') => "pipe?",
        Some(b'S') => "socket?",
        Some(b'b') => "blockdev?",
        Some(b'c') => "chardev?",
        Some(b'r' | b'R') => "readable?",
        Some(b'w' | b'W') => "writable?",
        Some(b'x' | b'X') => "executable?",
        Some(b'o' | b'O') => "owned?",
        Some(b'g') => "setgid?",
        Some(b'u') => "setuid?",
        Some(b'k') => "sticky?",
        Some(b's') => "size?",
        Some(b'z') => "zero?",
        Some(b'M') => "mtime",
        Some(b'A') => "atime",
        Some(b'C') => "ctime",
        _ => {
            let shown = match byte {
                Some(b) => format!("?{}", b as char),
                None => cmd.inspect_string(),
            };
            return Err(crate::builtins::arg_error!("unknown command {shown}"));
        }
    };
    crate::dispatch::send_value(
        &RubyValue::Class(zeo_abi::FILE_CLASS),
        Symbol::intern(file_method),
        std::slice::from_ref(path),
        None,
    )
}

/// Run the (user-overridable) `initialize_copy` hook on a freshly
/// shallow-copied object, with the original as its argument -- real Ruby's
/// `clone`/`dup` contract. Object's default hook is a no-op; a user
/// override (e.g. deep-copying a shared member) runs here.
/// `dup`/`clone`'s own hooks. CRuby calls `initialize_dup` from `dup` and
/// `initialize_clone` from `clone`, and BOTH default to calling
/// `initialize_copy` -- so a class overriding only `initialize_copy` still
/// sees it, while one overriding `initialize_dup` sees the more specific hook
/// too. Calling `initialize_copy` directly skipped the specific pair
/// entirely.
///
/// `freeze` is `clone`'s keyword, forwarded as the trailing options Hash the
/// hook's `freeze:` parameter binds.
fn copy_via_hook(
    original: &RubyValue,
    copy: RubyValue,
    hook: &str,
    freeze: Option<bool>,
) -> Result<RubyValue, Signal> {
    let hook = Symbol::intern(hook);
    if crate::dispatch::responds_to(copy.class_id(), hook, true) {
        let mut args = vec![original.clone()];
        if let Some(f) = freeze {
            args.push(RubyValue::Hash(crate::collections::hash_new(vec![(
                RubyValue::Symbol(Symbol::intern("freeze")),
                RubyValue::Bool(f),
            )])));
        }
        // The depth marker tells a native `initialize_copy` row this receiver
        // is dup/clone's FRESH copy -- CRuby's init_copy fills an
        // uninitialized allocation there, a job `dup_object` already did, so
        // the row must accept it while the direct spelling on a built
        // receiver keeps refusing (`Time#dup` raised through its own row).
        COPY_HOOK_DEPTH.with(|d| d.set(d.get() + 1));
        let sent = crate::dispatch::send_value(&copy, hook, &args, None);
        COPY_HOOK_DEPTH.with(|d| d.set(d.get() - 1));
        sent?;
    }
    Ok(copy)
}

thread_local! {
    static COPY_HOOK_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Whether the current call sits inside `dup`/`clone`'s copy-hook send --
/// the receiver is the fresh copy, not a built object. See `copy_via_hook`.
pub fn in_copy_hook() -> bool {
    COPY_HOOK_DEPTH.with(|d| d.get()) > 0
}

/// The `require`/`require_relative` runtime body, shared by the Kernel rows
/// above and `Ractor._require`. Whole-program AOT already spliced every
/// compile-time-resolvable require, so the only calls that land here are
/// genuinely dynamic (a computed path, or `load`). zeo has no runtime Ruby
/// loader, so an actually-executed dynamic load raises CRuby's LoadError
/// shape -- honest, and rescuable by `begin; require dyn; rescue LoadError`
/// -- rather than a silent no-op. A non-String-convertible argument raises
/// the same TypeError CRuby's path coercion does.
/// Whether an Object-backed value's class owns a VALUE `eql?` in CRuby --
/// the exceptions to `Object#eql?`'s identity that zeo serves from this
/// one row rather than from a row of their own.
///
/// MEASURED against the oracle rather than reasoned about, because the
/// split does not follow from anything zeo can see: `Time`, `Date`,
/// `Pathname`, `Set` and `BigDecimal` answer `eql?` by VALUE, while
/// `FFI::Pointer` and an `Exception` -- both of which answer `==` by value
/// -- answer `eql?` by identity. `Etc::Passwd`/`Etc::Group` and
/// `Process::Tms` are Structs, which own a value `eql?` like every other.
///
/// `tests/native_object_protocol.rb` pins the whole table, so a class that
/// lands on the wrong side of it shows up there rather than in a Hash
/// lookup that quietly misses.
fn owns_value_eql(recv: &RubyValue) -> bool {
    matches!(
        recv.class_id(),
        zeo_abi::TIME_CLASS
            | zeo_abi::PROCESS_TMS_CLASS
            | zeo_abi::SET_CLASS
            | zeo_abi::SET_CORE_SET_CLASS
            | zeo_abi::DATE_CLASS
            | zeo_abi::DATETIME_CLASS
            | zeo_abi::PATHNAME_CLASS
            | zeo_abi::BIGDECIMAL_CLASS
            | zeo_abi::ETC_PASSWD_CLASS
            | zeo_abi::ETC_GROUP_CLASS
    )
}

/// `caller`/`caller_locations`' shared `(start, length)` window over the frame
/// list: no argument starts at 1 (skipping the caller's own frame, since these
/// builtins push none); an Integer `start` with an optional `length`; or a
/// Range, whose bounds mean the same thing.
fn caller_window(start: Option<&RubyValue>, length: Option<&RubyValue>) -> (usize, Option<usize>) {
    match (start, length) {
        (None, _) => (1, None),
        (Some(RubyValue::Int(s)), len) => {
            let length = match len {
                Some(RubyValue::Int(l)) => Some((*l).max(0) as usize),
                _ => None,
            };
            ((*s).max(0) as usize, length)
        }
        (Some(RubyValue::Range(__rg)), _) => {
            let (s, e, excl) = __rg.parts();
            let lo = match s {
                Some(RubyValue::Int(v)) => (*v).max(0) as usize,
                _ => 0,
            };
            let hi = match e {
                Some(RubyValue::Int(v)) => {
                    Some(((*v).max(0) as usize).saturating_add(usize::from(!excl)))
                }
                _ => None,
            };
            (lo, hi.map(|h| h.saturating_sub(lo)))
        }
        _ => (1, None),
    }
}

/// `Kernel#sleep(seconds)` -- a genuinely interruptible wait on the
/// thread's own ctx condvar (a `Thread#raise`/`#kill` wakes it
/// immediately, delivered by the `check_ints` on the wake path); returns
/// the rounded seconds ACTUALLY slept. `sleep` with NO duration parks
/// until an interrupt arrives.
pub(crate) fn sleep_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let secs = match args.first() {
        Some(RubyValue::Int(n)) if *n >= 0 => Some(*n as f64),
        Some(RubyValue::Float(f)) if *f >= 0.0 => Some(*f),
        None => None,
        // A NEGATIVE number is an ArgumentError about the interval, not a
        // TypeError about the type: the value converted fine, it is just
        // out of range. Folded in with the non-numeric arm, it reported
        // `can't convert Integer into time interval` for `sleep(-1)`.
        Some(RubyValue::Int(_) | RubyValue::Float(_)) => {
            return Err(arg_error!("time interval must not be negative"));
        }
        Some(other) => {
            return Err(type_error!(
                "can't convert {} into time interval",
                crate::builtins::class_name_of(other)
            ));
        }
    };
    let ctx = crate::gvl::install_ctx();
    crate::thread::register_current_ctx(&ctx);
    let started = std::time::Instant::now();
    let deadline = secs.map(|s| started + std::time::Duration::from_secs_f64(s));
    // Park (armed Gvl released; free otherwise) until the deadline,
    // re-sleeping the remainder after any wake that check_ints doesn't
    // turn into a delivered kill/raise -- a 100ms quantum tick or other
    // spurious wake must not cut the sleep short. No deadline means only
    // an interrupt ever exits the loop.
    loop {
        // BEFORE parking, not only after: `Thread#kill`/`#raise` posted while
        // the target was still starting up finds no ctx to wake (this call
        // registers it), so a check only on the wake path would sit out the
        // whole sleep before noticing.
        crate::blocking_checkpoint()?;
        let remaining = match deadline {
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    break;
                }
                Some(d - now)
            }
            None => None,
        };
        crate::gvl::without_gvl(|| ctx.sleep(remaining));
    }
    Ok(RubyValue::Int(
        started.elapsed().as_secs_f64().round() as i64
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kernel's `ruby_module!` methods have mangled fn names, so tests reach
    /// them through the registered instance lookup (as real dispatch does).
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::KERNEL_CLASS)
            .expect("Kernel is registered")
            .instance
            .as_ref()
            .expect("Kernel has instance methods")
            .lookup)(name)
        .unwrap_or_else(|| panic!("Kernel#{name} is defined"))
    }

    /// The `Kernel` rows CRuby names parameters on. Every other Kernel row
    /// reports the anonymous descriptor, which is also ruby's answer.
    ///
    /// `require` is the asymmetric one: `Kernel#require` names its parameter
    /// and `Kernel.require` does not, because CRuby defines the two
    /// separately -- which is why they are two defs here rather than one
    /// `module_function`.
    #[test]
    fn the_named_kernel_rows_report_rubys_own_signature() {
        fn render(rows: Option<crate::builtins::ParamRows>) -> String {
            let Some(rows) = rows else {
                return "<none>".to_string();
            };
            let body: Vec<String> = rows
                .iter()
                .map(|(k, n)| {
                    use crate::method_meta::ParamKind;
                    let k = match k {
                        ParamKind::Req => "req",
                        ParamKind::Opt => "opt",
                        ParamKind::Rest => "rest",
                        ParamKind::KeyReq => "keyreq",
                        ParamKind::Key => "key",
                        ParamKind::KeyRest => "keyrest",
                        ParamKind::Block => "block",
                    };
                    match n {
                        Some(n) => format!("[:{k}, :{n}]"),
                        None => format!("[:{k}]"),
                    }
                })
                .collect();
            format!("[{}]", body.join(", "))
        }
        let t = crate::builtins::registered_table(zeo_abi::KERNEL_CLASS).expect("Kernel");
        let inst = t.instance.as_ref().expect("Kernel instance rows");
        let cls = t.class.as_ref().expect("Kernel class rows");
        let want: &[(&str, &str, i64)] = &[
            ("Float", "[[:req, :arg], [:key, :exception]]", -2),
            (
                "Integer",
                "[[:req, :arg], [:opt, :base], [:key, :exception]]",
                -2,
            ),
            ("Pathname", "[[:req, :path]]", 1),
            ("clone", "[[:key, :freeze]]", -1),
            ("pp", "[[:rest, :objs]]", -1),
            ("require", "[[:req, :path]]", 1),
            (
                "warn",
                "[[:rest, :msgs], [:key, :uplevel], [:key, :category]]",
                -1,
            ),
        ];
        let mut bad: Vec<String> = Vec::new();
        for (name, params, arity) in want {
            let got = (render((inst.params)(name)), (inst.arity)(name));
            if got.0 != *params || got.1 != Some(*arity) {
                bad.push(format!("Kernel#{name}: {got:?} want {params} / {arity}"));
            }
        }
        assert!(bad.is_empty(), "{bad:?}");
        // The class-side `require` reports no name, and answers the same body.
        assert_eq!(render((cls.params)("require")), "<none>");
        assert_eq!((cls.arity)("require"), Some(1));
        assert!((cls.lookup)("require").is_some());
    }

    #[test]
    fn eql_requires_same_class_and_equality() {
        let t = imethod("eql?")(&RubyValue::Int(1), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(t, RubyValue::Bool(true)));
        let f = imethod("eql?")(&RubyValue::Int(1), &[RubyValue::Float(1.0)], None).unwrap();
        assert!(matches!(f, RubyValue::Bool(false)));
    }

    #[test]
    fn to_s_and_inspect_render_like_puts_and_p() {
        // KERNEL's rows, which are `rb_any_to_s`/`rb_obj_inspect` -- the class
        // and an address, for EVERY receiver. Oracle-checked against ruby
        // 4.0.6: `Kernel.instance_method(:inspect).bind(nil).call` answers
        // `"#<NilClass:0x...>"`, not `"nil"`. `nil.inspect` is `"nil"`
        // because NilClass carries its own row, which is a different
        // question -- and the one a `super` out of a reopened row resumes
        // past.
        // The CLASS NAME is not asserted: no registry is installed in a bare
        // unit test, so `class_name` falls back to "Object". The differential
        // probe against ruby 4.0.6 covers the name; the shape is what this
        // test can see.
        for name in ["to_s", "inspect"] {
            let v = imethod(name)(&RubyValue::Nil, &[], None).unwrap();
            let RubyValue::Str(v) = v else { panic!() };
            let v = v.lock().to_utf8_lossy().into_owned();
            assert!(
                v.starts_with("#<") && v.contains(":0x") && v.ends_with('>'),
                "Kernel#{name} answered {v}"
            );
        }
    }

    #[test]
    fn itself_returns_the_receiver_and_freeze_reports_frozen() {
        assert!(matches!(
            imethod("itself")(&RubyValue::Int(7), &[], None).unwrap(),
            RubyValue::Int(7)
        ));
        assert!(matches!(
            imethod("frozen?")(&RubyValue::Int(7), &[], None).unwrap(),
            RubyValue::Bool(true) // immediates are frozen
        ));
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        assert!(matches!(
            imethod("frozen?")(&s, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
        imethod("freeze")(&s, &[], None).unwrap();
        assert!(matches!(
            imethod("frozen?")(&s, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }
}
