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

use crate::builtins::inherited_row;
use crate::builtins::{arg_error, block_or_enum, local_jump_error, need_block, type_error};
use crate::{RubyValue, Signal, Symbol};
use zeo_macros::ruby_module;

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
fn own_singleton_names(recv: &RubyValue) -> Vec<Symbol> {
    match recv {
        RubyValue::Class(cid) => crate::dispatch::public_class_method_names(*cid, false),
        // Both report the PUBLIC surface. `def obj.x` is public as written, so
        // the filter only ever removes a name the singleton class was
        // explicitly told to hide -- and that mark lives on the singleton
        // class, which exists only if something named it.
        _ => {
            let names = crate::runtime_meta::singleton_method_names(recv);
            let Some(sclass) = crate::runtime_meta::minted_singleton_class(recv) else {
                return names;
            };
            names
                .into_iter()
                .filter(|&n| {
                    crate::dispatch::value_singleton_visibility(sclass, n)
                        != crate::dispatch::MethodVisibility::Private
                })
                .collect()
        }
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
    // is `dispatch::send_value_public_in`. The rustc backend folds that gate
    // into the call site and so reaches this row only where it did not fold;
    // the CLIF backend has no such fold and reaches it always -- so the rule
    // lives HERE, where both find it.
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
        crate::builtins::file::lookup_class("open").unwrap()(
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
        require_feature(recv, std::slice::from_ref(arg1), None)
    }
    module_function def "require" as require_feature (_recv, arg1) {
        dynamic_require(arg1)
    }
    // Resolved against the CALLING file's directory, which a compiled binary
    // still knows -- see `dynamic_require_relative`.
    module_function def "require_relative" (_recv, arg1) {
        dynamic_require_relative(arg1)
    }
    private def "pp"(_recv, *args, &_block) {
        kernel_pp(args)
    }
    module_function def "warn"(_recv, *args, &_block) {
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
        for m in std::iter::once(first).chain(rest) {
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
                    return Err(type_error!(
                        "wrong argument type {} (expected binding)",
                        crate::dispatch::class_name(b.class_id())
                            .unwrap_or_else(|| "Object".to_string())
                    ));
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
    // codegen's `emit_raise`, following CRuby's `rb_make_exception`: the
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
        let mut window: Vec<RubyValue> = all[start..]
            .iter()
            .map(|(f, l, m)| crate::builtins::backtrace_location::location_new(f, *l, m))
            .collect();
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
    module_function def "Integer" as kernel_integer (_recv, _arg, _base?, _opts?) {
        let _frame = conversion_frame("Kernel#Integer");
        with_exception_kw(__args, integer_impl)
    }
    // The second slot is the `exception:` hash -- see `Integer`'s note.
    module_function def "Float" as kernel_float (_recv, _arg, _opts?) {
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
    module_function def "Pathname"(_recv, arg) {
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
    def "clone"(recv, arg?) {
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
        Ok(RubyValue::Str(crate::string_new(recv.try_display_string()?)))
    }
    def "inspect" (recv) {
        Ok(RubyValue::Str(crate::string_new(recv.try_inspect_string()?)))
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
        // `Object#methods` returns public AND protected names.
        names.extend(crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::VisFilter::NotPrivate,
            true,
        ));
        Ok(syms_to_array(dedup_syms(names)))
    }
    def "public_methods"(recv, arg?) {
        let inherit = truthy_arg(arg);
        let mut names = Vec::new();
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
        let names = crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::VisFilter::Private,
            inherit,
        );
        Ok(syms_to_array(names))
    }
    def "protected_methods"(recv, arg?) {
        let inherit = truthy_arg(arg);
        let names = crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::VisFilter::Protected,
            inherit,
        );
        Ok(syms_to_array(names))
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

    // ---- the FOLDED intrinsics, as real rows.
    //
    // Each of these is compiled straight into the caller by
    // `codegen::call::kernel`, which is why `Kernel#printf(...)` has always
    // worked. Without a table row, though, `send(:printf, ...)` raised
    // NoMethodError, `method(:printf)` raised NameError and
    // `respond_to?(:printf, true)` answered false -- ruby answers all three.
    // The row calls the very function the fold calls, so the two forms cannot
    // diverge. `module_function`, which is how ruby has them: a PRIVATE
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
    module_function def "lambda"(_recv, &block) {
        let p = crate::builtins::need_block!(block);
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
            RubyValue::Proc(_) => Err(crate::dispatch::raise_error(
                "NotImplementedError",
                "set_trace_func is not supported: this build carries no trace hooks"
                    .to_string(),
            )),
            _ => Err(type_error!("trace_func needs to be Proc")),
        }
    }
    module_function def "syscall" arity 0 (_recv, *_args) {
        Err(crate::dispatch::raise_error(
            "NotImplementedError",
            "syscall() function is unimplemented on this machine".to_string(),
        ))
    }

    // The caller-scope intrinsics. Every direct or literal-`send` spelling
    // folds into the caller at compile time (`lower`'s rewrite,
    // `codegen::call::kernel`), so these rows exist for REFLECTION --
    // `respond_to?(:block_given?, true)`, `method(:binding)`, the private
    // NoMethodError for an explicit receiver -- and for the one spelling no
    // fold can serve: a `send` whose name is computed at runtime. A row
    // cannot see its caller's block or scope, so that spelling is refused
    // loudly rather than answered wrongly (the `set_trace_func` rule; see
    // tests/gaps/kernel_scope_intrinsics_dynamic_send.rb).
    module_function def "block_given?" | "iterator?" (_recv) {
        Err(crate::dispatch::raise_error(
            "NotImplementedError",
            "block_given? cannot be reached through a runtime-computed send: a method row cannot see the caller's block".to_string(),
        ))
    }
    module_function def "binding"(_recv) {
        Err(crate::dispatch::raise_error(
            "NotImplementedError",
            "binding cannot be reached through a runtime-computed send: a method row cannot see the caller's scope".to_string(),
        ))
    }
    module_function def "local_variables"(_recv) {
        Err(crate::dispatch::raise_error(
            "NotImplementedError",
            "local_variables cannot be reached through a runtime-computed send: a method row cannot see the caller's scope".to_string(),
        ))
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

/// `gets`/`readline`/`readlines` against `$stdin` -- see their rows.
fn stdin_send(name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let stdin = crate::globals::global_get(0, "$stdin");
    crate::dispatch::send_value(&stdin, Symbol::intern(name), args, None)
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
        crate::dispatch::send_value(&copy, hook, &args, None)?;
    }
    Ok(copy)
}

/// `Kernel#Integer(arg, base = nil)` -- CRuby's strict conversion: strings
/// allow surrounding whitespace, single underscores between digits, and
/// radix prefixes (`0x`/`0o`/`0b`, or a leading `0` octal when no base is
/// given); floats/rationals TRUNCATE toward zero; nil and everything else
/// is a TypeError. Message shapes oracle-verified.
/// Splits the `exception:` keyword off a Kernel conversion's arguments.
///
/// Every one of `Integer`/`Float`/`Rational`/`Complex` takes it, and it is a
/// KEYWORD -- never one of the value arguments -- so the trailing options Hash
/// comes off before the positional shape is read at all. Read as a positional
/// it became a base, a denominator or an imaginary part, which is how
/// `Integer("abc", exception: false)` used to raise about a Hash.
fn split_exception_kw(args: &[RubyValue]) -> (&[RubyValue], bool) {
    let Some(RubyValue::Hash(h)) = args.last() else {
        return (args, true);
    };
    let key = RubyValue::Symbol(crate::Symbol::intern("exception"));
    if !crate::collections::hash_has_key(h, &key) {
        return (args, true);
    }
    (
        &args[..args.len() - 1],
        crate::collections::hash_get(h, &key).truthy(),
    )
}

/// Runs a Kernel conversion under its `exception:` keyword: `false` answers
/// `nil` instead of raising, which is the entire point of the keyword. Only a
/// RAISE is swallowed -- a `break`/`throw` crossing the conversion still
/// propagates.
/// `Kernel#Integer` and its family are ordinary cfuncs in CRuby, which push a
/// control frame -- so a raise from inside one names it
/// (`f.rb:2:in 'Kernel#Integer'`) where a specialized instruction like
/// `opt_ltlt` names only the caller.
///
/// Pushed HERE and not at the dispatch boundary because codegen's fast path
/// calls this very function directly and never reaches that boundary
/// (`codegen::call::kernel`'s `row_fn`). `synthetic_c_frame`'s exact-repeat
/// dedupe keeps the DISPATCH route to one frame, not two.
fn conversion_frame(label: &'static str) -> crate::frames::CFrameGuard {
    crate::frames::synthetic_c_frame(label)
}

fn with_exception_kw(
    args: &[RubyValue],
    f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal>,
) -> Result<RubyValue, Signal> {
    let (positional, raising) = split_exception_kw(args);
    match f(positional) {
        Err(Signal::Raise(_)) if !raising => Ok(RubyValue::Nil),
        other => other,
    }
}

pub(crate) fn integer_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let base = match args.get(1) {
        None => None,
        Some(v) => Some(crate::builtins::convert::to_index(v)? as u32),
    };
    // A base only makes sense for a String argument -- CRuby raises rather than
    // silently ignoring it for an Integer/Float/etc.
    if base.is_some() && !matches!(args[0], RubyValue::Str(_)) {
        return Err(arg_error!("base specified for non string value"));
    }
    match &args[0] {
        RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(args[0].clone()),
        RubyValue::Float(f) => {
            if f.is_finite() {
                Ok(crate::builtins::integer::int_value(
                    num_bigint::BigInt::from(f.trunc() as i128),
                ))
            } else {
                Err(crate::dispatch::raise_error(
                    "FloatDomainError",
                    crate::RubyValue::Float(*f).to_display_string(),
                ))
            }
        }
        RubyValue::Rational(r) => Ok(crate::builtins::integer::int_value(&r.num / &r.den)),
        RubyValue::Str(s) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            // Base 0 is strtol's "detect from the prefix" -- exactly the
            // no-base rule (0x/0o/0b, leading-0 octal, else decimal).
            let base = base.filter(|&b| b != 0);
            parse_integer_strict(&text, base)
                .ok_or_else(|| arg_error!("invalid value for Integer(): {:?}", text))
        }
        // A `to_int` duck converts (CRuby tries to_int, then to_i); the
        // rest keep Kernel#Integer's own "can't convert" shape.
        other => match crate::builtins::convert::check_to_int(other)? {
            Some(n) => Ok(n),
            None => Err(type_error!(
                "can't convert {} into Integer",
                crate::builtins::convert_name_of(other)
            )),
        },
    }
}

/// The strict string parser `Integer()` and `String#to_i(base)` share:
/// optional whitespace/sign, radix prefix (honored when compatible with an
/// explicit base), single underscores between digits.
pub(crate) fn parse_integer_strict(text: &str, base: Option<u32>) -> Option<RubyValue> {
    let t = text.trim();
    let (negative, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let lower = t.to_ascii_lowercase();
    let (radix, digits) = if let Some(rest) = lower.strip_prefix("0x") {
        (16, rest.to_string())
    } else if let Some(rest) = lower.strip_prefix("0o") {
        (8, rest.to_string())
    } else if let Some(rest) = lower.strip_prefix("0b") {
        (2, rest.to_string())
    // `0d` is ruby's EXPLICIT decimal prefix, the fourth of the set. Without
    // it "0d19" fell through to the leading-zero octal rule and then failed on
    // the `d`.
    } else if let Some(rest) = lower.strip_prefix("0d") {
        (10, rest.to_string())
    } else if lower.len() > 1 && lower.starts_with('0') && base.is_none() {
        (8, lower[1..].to_string())
    } else {
        (base.unwrap_or(10), lower)
    };
    if let Some(b) = base
        && b != radix
        && !(b == 10 && radix == 10)
    {
        // An explicit base must agree with an explicit prefix.
        if radix != b {
            return None;
        }
    }
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
    {
        return None;
    }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
    // Only one sign is allowed, and it was already consumed above -- a residual
    // `+`/`-` (`"++7"`, `"+-7"`) is invalid, though `parse_bytes` would accept
    // a leading `+`.
    if clean.starts_with(['+', '-']) {
        return None;
    }
    let parsed = num_bigint::BigInt::parse_bytes(clean.as_bytes(), radix)?;
    Some(crate::builtins::integer::int_value(if negative {
        -parsed
    } else {
        parsed
    }))
}

/// `Kernel#Float(arg)` -- strict string parse (Rust's `f64::from_str`
/// covers Ruby's accepted forms incl. exponents; underscores stripped),
/// numerics via the tower's f64 view.
pub(crate) fn float_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match &args[0] {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
            Ok(RubyValue::Float(
                crate::builtins::numeric::num_to_f64_unchecked(&args[0]),
            ))
        }
        RubyValue::Str(s) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            let trimmed = text.trim();
            let invalid = || arg_error!("invalid value for Float(): {:?}", text);
            // Underscores are only legal BETWEEN two digits (hex digits for a
            // `0x` float): a leading/trailing/doubled `_`, or one adjacent to
            // `.`/`e`/`p`/a sign, is rejected (`"1__0"`, `"1_"`, `"1_e3"`).
            let Some(clean) = strip_valid_underscores(trimmed) else {
                return Err(invalid());
            };
            // Rust's parser accepts the words "inf"/"infinity"/"nan"; CRuby's
            // Float() does not (only numeric literals). A valid numeric string
            // never contains those substrings.
            let lower = clean.to_ascii_lowercase();
            if lower.contains("inf") || lower.contains("nan") {
                return Err(invalid());
            }
            clean
                .parse::<f64>()
                .ok()
                // C99 hex-float (`"0x1p4"` = 16.0), which `str::parse` rejects.
                .or_else(|| parse_hex_float(&clean))
                .map(RubyValue::Float)
                .ok_or_else(invalid)
        }
        // Anything else goes through the `to_f` protocol, CRuby's
        // `rb_convert_type_with_id(val, T_FLOAT, "Float", idTo_f)`. This is what
        // lets `Float(obj)` and every `Numeric` default built on it answer for a
        // user `class Temp < Numeric` that defines only `to_f`. `nil` has no
        // `to_f` for this purpose -- CRuby rejects it before asking.
        other => {
            let to_f = crate::Symbol::intern("to_f");
            let refuse = || {
                type_error!(
                    "can't convert {} into Float",
                    crate::builtins::convert_name_of(other)
                )
            };
            if matches!(other, RubyValue::Nil)
                || !crate::dispatch::responds_to(other.class_id(), to_f, false)
            {
                return Err(refuse());
            }
            match crate::dispatch::send_value(other, to_f, &[], None)? {
                f @ RubyValue::Float(_) => Ok(f),
                // A `to_f` that answers something else is a broken conversion,
                // not a Float -- CRuby reports the same refusal.
                _ => Err(refuse()),
            }
        }
    }
}

/// Validates that every `_` in a `Float()` string sits between two digits and
/// returns the string with the underscores removed; `None` if any is misplaced.
/// A `0x`-prefixed value uses hex-digit adjacency (so `0x1_1` is fine) while a
/// decimal value uses `0-9` (so the `e` in `1_e3` doesn't count as a digit).
fn strip_valid_underscores(s: &str) -> Option<String> {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let is_hex = body.starts_with("0x") || body.starts_with("0X");
    let is_digit = |c: u8| {
        if is_hex {
            c.is_ascii_hexdigit()
        } else {
            c.is_ascii_digit()
        }
    };
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let prev_ok = i > 0 && is_digit(bytes[i - 1]);
            let next_ok = i + 1 < bytes.len() && is_digit(bytes[i + 1]);
            if !(prev_ok && next_ok) {
                return None;
            }
        }
    }
    Some(s.chars().filter(|c| *c != '_').collect())
}

/// Parse a C99 hexadecimal float (`[±]0x<hex>.<hex>p<dec-exp>`, the exponent a
/// power of TWO), which `str::parse::<f64>` rejects: `"0x1p4"` -> 16.0,
/// `"0x1.8p1"` -> 3.0. `None` if the string isn't this shape.
fn parse_hex_float(s: &str) -> Option<f64> {
    let t = s.trim();
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))?;
    // The binary exponent `p<dec>` is optional: `"0xa"` is 10.0 (exponent 0).
    let (mantissa, exp): (&str, i32) = match t.find(['p', 'P']) {
        Some(idx) => (&t[..idx], t[idx + 1..].parse().ok()?),
        None => (t, 0),
    };
    let (int_str, frac_str) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int_str.is_empty() && frac_str.is_empty() {
        return None;
    }
    let mut value = 0.0f64;
    for c in int_str.chars() {
        value = value * 16.0 + c.to_digit(16)? as f64;
    }
    let mut scale = 1.0 / 16.0;
    for c in frac_str.chars() {
        value += c.to_digit(16)? as f64 * scale;
        scale /= 16.0;
    }
    let result = value * 2f64.powi(exp);
    Some(if neg { -result } else { result })
}

/// `Kernel#Rational(num, den = 1)` -- exact components only (string forms
/// are a documented scope-cut).
pub(crate) fn rational_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let exact = |v: &RubyValue| -> Result<(num_bigint::BigInt, num_bigint::BigInt), Signal> {
        match v {
            RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
                Ok(crate::builtins::rational::as_ratio(v))
            }
            // A Float contributes its EXACT dyadic value (`Rational(0.3)` is
            // the true `5404.../18014...`, not `3/10`).
            RubyValue::Float(f) => Ok(crate::builtins::float::float_exact_parts(*f)),
            // A String is PARSED as a rational literal (`"3/4"`, `"-5/2"`,
            // `"2.5"`, `"6"`) -- its DECIMAL value, not its Float value, so
            // `"2.5"` is exactly `5/2`.
            RubyValue::Str(s) => parse_rational_string(&s.lock().to_utf8_lossy()),
            other => Err(type_error!(
                "can't convert {} into Rational",
                crate::builtins::convert_name_of(other)
            )),
        }
    };
    let (nn, nd) = exact(&args[0])?;
    let (dn, dd) = match args.get(1) {
        Some(d) => exact(d)?,
        None => (num_bigint::BigInt::from(1), num_bigint::BigInt::from(1)),
    };
    // (nn/nd) / (dn/dd) == (nn*dd) / (nd*dn)
    crate::builtins::rational::rational_new(nn * dd, nd * dn)
}

/// `Kernel#Complex(real, imag = 0)`. A single String argument is parsed as a
/// complex literal (`"2+3i"`, `"3"`, `"-i"`).
pub(crate) fn complex_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    use crate::builtins::complex;

    // A `nil` in either position is refused up front, before either
    // component is examined, so it reports the conversion rather than
    // `Complex.rect`'s "not a real" (CRuby `nucomp_convert`).
    if matches!(args[0], RubyValue::Nil) || matches!(args.get(1), Some(RubyValue::Nil)) {
        return Err(type_error!("can't convert nil into Complex"));
    }
    // A String component is parsed, in EITHER position.
    let parse = |v: &RubyValue| -> Result<RubyValue, Signal> {
        match v {
            RubyValue::Str(s) => {
                let (real, imag) = parse_complex_string(&s.lock().to_utf8_lossy())?;
                complex::complex_new(real, imag)
            }
            other => Ok(other.clone()),
        }
    };
    // A real-valued Complex contributes its own real part -- so
    // `Complex(Complex(3, 0), 4)` is `(3+4i)`, not a nested component.
    let unwrap_real = |v: RubyValue| match &v {
        RubyValue::Complex(c) if complex::is_exact_zero(&c.imag) => c.real.clone(),
        _ => v,
    };
    let a1 = unwrap_real(parse(&args[0])?);
    let a2 = match args.get(1) {
        Some(v) => Some(unwrap_real(parse(v)?)),
        None => None,
    };

    // A Complex that survived the unwrap passes through whole, provided the
    // imaginary argument would contribute nothing.
    if matches!(a1, RubyValue::Complex(_)) && a2.as_ref().is_none_or(complex::is_exact_zero) {
        return Ok(a1);
    }
    let Some(a2) = a2 else {
        // One argument: a NON-real numeric is already the answer, and a
        // non-numeric converts through `#to_c`.
        return match complex::is_real_numeric(&a1)? {
            Some(false) => Ok(a1),
            Some(true) => complex::complex_new(a1, RubyValue::Int(0)),
            None if crate::dispatch::responds_to_value(&a1, Symbol::intern("to_c"), false) => {
                crate::dispatch::send_value(&a1, Symbol::intern("to_c"), &[], None)
            }
            None => Err(type_error!(
                "can't convert {} into Complex",
                crate::builtins::convert_name_of(&a1)
            )),
        };
    };
    // Two arguments, either of them non-real: the pair means `a1 + a2*i`,
    // which is arithmetic on the components, not a rectangular build.
    if matches!(complex::is_real_numeric(&a1)?, Some(r1) if !r1)
        || matches!(complex::is_real_numeric(&a2)?, Some(r2) if !r2)
    {
        let unit = complex::complex_new(RubyValue::Int(0), RubyValue::Int(1))?;
        let scaled = crate::dispatch::send_value(&a2, Symbol::intern("*"), &[unit], None)?;
        return crate::dispatch::send_value(&a1, Symbol::intern("+"), &[scaled], None);
    }
    complex::complex_new(complex::real_check(&a1)?, complex::real_check(&a2)?)
}

/// The `ArgumentError` CRuby's numeric-string converters raise on an
/// unparseable value: `invalid value for convert(): "<original>"`.
fn convert_error(original: &str) -> Signal {
    arg_error!("invalid value for convert(): {original:?}")
}

/// Parse a rational literal string to its `(numerator, denominator)` DECIMAL
/// value: `"3/4"` -> `(3, 4)`, `"2.5"` -> `(25, 10)` (exactly `5/2`, not the
/// Float value), `"6"` -> `(6, 1)`. Leading/trailing whitespace and a sign are
/// allowed. A `"n/0"` denominator is ZeroDivisionError, like the numeric form.
fn parse_rational_string(s: &str) -> Result<(num_bigint::BigInt, num_bigint::BigInt), Signal> {
    use num_bigint::BigInt;
    let t = s.trim();
    // A scientific EXPONENT scales the mantissa exactly -- `Rational("1.5e2")`
    // is `(150/1)`, not the float 1.5 times 100. Peeled first so the mantissa
    // reaches the decimal branch below unchanged; a `/` form has no exponent.
    if !t.contains('/')
        && let Some(at) = t.rfind(['e', 'E'])
        && at > 0
    {
        let (mantissa, exp) = t.split_at(at);
        let exp: i32 = exp[1..].parse().map_err(|_| convert_error(s))?;
        let (mut num, mut den) = parse_rational_string(mantissa)?;
        let scale = BigInt::from(10).pow(exp.unsigned_abs());
        if exp >= 0 {
            num *= scale;
        } else {
            den *= scale;
        }
        return Ok((num, den));
    }
    if let Some((n, d)) = t.split_once('/') {
        let num: BigInt = n.trim().parse().map_err(|_| convert_error(s))?;
        let den: BigInt = d.trim().parse().map_err(|_| convert_error(s))?;
        if den == BigInt::from(0) {
            return Err(crate::dispatch::raise_error(
                "ZeroDivisionError",
                "divided by 0".to_string(),
            ));
        }
        Ok((num, den))
    } else if let Some((int_part, frac_part)) = t.split_once('.') {
        let neg = int_part.trim_start().starts_with('-');
        let int_digits: String = int_part.chars().filter(char::is_ascii_digit).collect();
        let frac_digits: String = frac_part.chars().filter(char::is_ascii_digit).collect();
        if int_digits.is_empty() && frac_digits.is_empty() {
            return Err(convert_error(s));
        }
        let mut num: BigInt = format!("{int_digits}{frac_digits}")
            .parse()
            .map_err(|_| convert_error(s))?;
        if neg {
            num = -num;
        }
        Ok((num, BigInt::from(10).pow(frac_digits.len() as u32)))
    } else {
        Ok((t.parse().map_err(|_| convert_error(s))?, BigInt::from(1)))
    }
}

/// Parse a complex literal string to `(real, imag)` values: `"2+3i"`,
/// `"1+2i"`, `"3"` (-> `(3, 0)`), `"-i"` (-> `(0, -1)`), `"4i"` (-> `(0, 4)`).
/// Each component is an Integer when it has no decimal point, else a Float.
fn parse_complex_string(s: &str) -> Result<(RubyValue, RubyValue), Signal> {
    let t = s.trim();
    let num = |part: &str| -> Result<RubyValue, Signal> {
        if part.contains('.') {
            part.parse::<f64>()
                .map(RubyValue::Float)
                .map_err(|_| convert_error(s))
        } else {
            part.parse::<i64>()
                .map(RubyValue::Int)
                .map_err(|_| convert_error(s))
        }
    };
    // The imaginary coefficient: an empty/sign-only string is the unit `±1`.
    let imag = |part: &str| -> Result<RubyValue, Signal> {
        match part {
            "" | "+" => Ok(RubyValue::Int(1)),
            "-" => Ok(RubyValue::Int(-1)),
            other => num(other),
        }
    };
    let Some(body) = t.strip_suffix('i').or_else(|| t.strip_suffix('I')) else {
        // No imaginary unit -> a pure real value.
        return Ok((num(t)?, RubyValue::Int(0)));
    };
    // Split real+imag at the sign joining them (not a leading sign, and not an
    // exponent sign after `e`/`E`).
    let split = body.char_indices().rev().find(|&(idx, c)| {
        (c == '+' || c == '-')
            && idx != 0
            && !matches!(body.as_bytes().get(idx - 1), Some(b'e' | b'E'))
    });
    match split {
        Some((idx, _)) => Ok((num(&body[..idx])?, imag(&body[idx..])?)),
        None => Ok((RubyValue::Int(0), imag(body)?)),
    }
}

/// `Kernel#String(arg)` -- `to_s` (the `to_str`-first nuance is invisible
/// for builtin receivers).
pub(crate) fn string_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    // `to_str` FIRST, then `to_s` -- `rb_f_string` tries the strict conversion
    // and only falls back to the display form, so an object defining `to_str`
    // is converted by it rather than stringified.
    if let Some(s) = crate::builtins::convert::check_to_str(&args[0])? {
        return Ok(s);
    }
    Ok(RubyValue::Str(crate::string_new(
        args[0].to_display_string(),
    )))
}

/// `Kernel#Array(arg)`: nil -> [], Array -> itself, Hash -> assoc pairs,
/// Range -> to_a, anything else -> [arg]. (`to_ary`/`to_a` protocol probes
/// on user objects are a documented scope-cut, beyond the value-subclass
/// case below, which IS an Array.)
pub(crate) fn array_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if let Some(a) = crate::builtins::convert::check_to_ary(&args[0])? {
        return Ok(a);
    }
    // `to_ary` FIRST, then `to_a` -- `rb_Array` tries both, in that order, so
    // an object defining only `to_a` still converts.
    let to_a = crate::Symbol::intern("to_a");
    if crate::dispatch::responds_to_value(&args[0], to_a, false)
        && let RubyValue::Array(_) = crate::dispatch::send_value(&args[0], to_a, &[], None)?
    {
        return crate::dispatch::send_value(&args[0], to_a, &[], None);
    }
    Ok(match &args[0] {
        RubyValue::Nil => RubyValue::Array(crate::array_new(Vec::new())),
        RubyValue::Array(_) => args[0].clone(),
        RubyValue::Hash(h) => RubyValue::Array(crate::array_new(
            h.lock()
                .values()
                .map(|(k, v)| RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])))
                .collect(),
        )),
        RubyValue::Range(..) => {
            crate::builtins::enumerable::enumerable_send(&args[0], "to_a", &[], None)
                .expect("Enumerable implements to_a")?
        }
        other => RubyValue::Array(crate::array_new(vec![other.clone()])),
    })
}

/// `Kernel#Hash(arg)`: nil/[] -> {}, Hash -> itself, else TypeError.
pub(crate) fn hash_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match &args[0] {
        RubyValue::Nil => Ok(RubyValue::Hash(crate::hash_new(Vec::new()))),
        RubyValue::Array(a) if a.lock().is_empty() => {
            Ok(RubyValue::Hash(crate::hash_new(Vec::new())))
        }
        RubyValue::Hash(_) => Ok(args[0].clone()),
        // `to_hash` converts, as `rb_Hash` does; only a value with none is the
        // TypeError.
        other => match crate::builtins::convert::check_to_hash(other)? {
            Some(h) => Ok(h),
            None => Err(type_error!(
                "can't convert {} into Hash",
                crate::builtins::convert_name_of(other)
            )),
        },
    }
}

/// `Kernel#puts`: zero args print one newline; arrays flatten recursively,
/// each scalar on its own line (nil renders empty) -- CRuby's exact rules.
/// Routed through whatever `$stdout` currently holds (default: the
/// `STDOUT` singleton) -- `$stdout = STDERR` or any duck-typed writer
/// redirects the whole print family. See `builtins::io` for the rendering
/// and write plumbing.
pub fn kernel_puts(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let mut buf = Vec::new();
    // A raising `to_s` mid-render still FLUSHES what rendered before it --
    // CRuby writes line by line, so `puts [1, raiser]` prints "1" and then
    // raises (oracle-verified). Rendering into one buffer and flushing
    // before propagating reproduces that observable order.
    let rendered = crate::builtins::io::render_puts(args, &mut buf);
    crate::builtins::io::write_bytes(&crate::builtins::io::current_stdout(), &buf)?;
    rendered?;
    Ok(RubyValue::Nil)
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

pub(crate) fn dynamic_require(arg1: &RubyValue) -> Result<RubyValue, crate::Signal> {
    let path = crate::builtins::convert::to_rstr(arg1)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    // ...unless the front end already spliced this very file, in which case
    // Ruby's own answer for an already-loaded feature -- `false` -- is both
    // correct and what the caller expects.
    if feature_already_loaded(&path) {
        return Ok(RubyValue::Bool(false));
    }
    // A unit the front end compiled in for exactly this case -- a require whose
    // target only the running program knows. See `crate::features`.
    if let Some(result) = crate::features::load_feature(&path) {
        return result.map(RubyValue::Bool);
    }
    // ...and, failing that, the file on DISK: a target under no compile-time
    // root at all (`$LOAD_PATH.unshift(dir); require "x"`, or an absolute
    // path). Compiled where it is found, by the same compiler an `eval`
    // reaches.
    if let Some(result) = crate::features::load_from_disk(&path, 0, false) {
        return result.map(RubyValue::Bool);
    }
    // `#path` carries the feature as WRITTEN. CRuby absolutizes it for
    // `require_relative` only, against the calling file's directory -- a
    // compiled binary has no such directory, so the argument stands.
    // A feature zeo DECLINES says so; everything else keeps CRuby's bare
    // wording. Shared with the compiler's loader through the ABI, the only
    // thing the two sides agree on.
    Err(missing_feature_error(&path))
}

/// The `require_relative` runtime body: CRuby resolves the path against the
/// CALLING file's directory (`rb_f_require_relative`), and so does zeo -- the
/// innermost compiled frame carries the spliced file's canonical path, and
/// the compiled-in units register under exactly that absolutized spelling.
/// That is what makes an `autoload`-DSL helper's `require_relative.call(f)`
/// land on the unit for the file `f` names (rspec-support's
/// `define_optimized_require_for_rspec` is the corpus case). An argument that
/// cannot be absolutized (already absolute, or no compiled frame below)
/// resolves exactly like `require`; a miss raises with the ABSOLUTIZED path,
/// which is the message shape CRuby's `require_relative` has.
pub(crate) fn dynamic_require_relative(arg1: &RubyValue) -> Result<RubyValue, crate::Signal> {
    let path = crate::builtins::convert::to_rstr(arg1)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    let absolutized = (!path.starts_with('/'))
        .then(crate::frames::current_location)
        .flatten()
        .and_then(|(file, _)| {
            // The MAIN file's frames carry its path AS GIVEN (`__FILE__`'s
            // rule), so a relative one resolves against the process cwd first
            // -- units register under canonical absolute spellings, and this
            // is how `zeo tests/foo.rb` finds `tests/foo/…` targets.
            let file = std::path::Path::new(file);
            let file = if file.is_absolute() {
                file.to_path_buf()
            } else {
                std::env::current_dir().ok()?.join(file)
            };
            Some(lexical_join(file.parent()?, &path))
        });
    let Some(abs) = absolutized else {
        return dynamic_require(arg1);
    };
    if feature_already_loaded(&abs) {
        return Ok(RubyValue::Bool(false));
    }
    if let Some(result) = crate::features::load_feature(&abs) {
        return result.map(RubyValue::Bool);
    }
    // The as-written spelling second: a unit registered under its bare
    // feature name (`require_relative "version"` next to a load-path root)
    // still resolves, matching the compiler's own root-relative fallback.
    if feature_already_loaded(&path) {
        return Ok(RubyValue::Bool(false));
    }
    if let Some(result) = crate::features::load_feature(&path) {
        return result.map(RubyValue::Bool);
    }
    // The ABSOLUTIZED spelling on disk -- `require_relative` resolves
    // against the calling file's directory, which is what the frame carries.
    if let Some(result) = crate::features::load_from_disk(&abs, 0, false) {
        return result.map(RubyValue::Bool);
    }
    Err(missing_feature_error(&abs))
}

/// `dir` + `rel`, normalized LEXICALLY (`.`/`..` folded without touching the
/// filesystem -- the file need not exist on the machine the binary runs on).
fn lexical_join(dir: &std::path::Path, rel: &str) -> String {
    let mut parts: Vec<&str> = dir
        .to_str()
        .unwrap_or_default()
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    for c in rel.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    format!("/{}", parts.join("/"))
}

/// The `LoadError` a feature that is not compiled in raises, carrying `#path`
/// and -- when the front end reached the file but could not lower it -- the
/// reason it declined. Shared by `require` and by `Module#autoload`, which
/// stands in for the same load.
pub(crate) fn missing_feature_error(path: &str) -> crate::Signal {
    let msg = match zeo_abi::declined_feature_reason(path)
        .or_else(|| crate::features::decline_reason(path))
    {
        Some(reason) => format!("cannot load such file -- {path}: {reason}"),
        None => format!("cannot load such file -- {path}"),
    };
    let sig = crate::dispatch::raise_error("LoadError", msg);
    if let crate::signal::Signal::Raise(exc) = &sig {
        crate::builtins::exception::set_load_error_path(exc, path);
    }
    sig
}

/// Whether `path` names a file the front end already spliced -- that is,
/// whether it is in `$LOADED_FEATURES` (see `globals::seed_loaded_features`).
///
/// Compared as a suffix on a path boundary, not for equality. The seeded
/// entries are canonical absolute paths, while a dynamic require may name the
/// file relatively (`require_relative "smtp/auth_plain"`, with or without
/// `.rb`). Suffix matching finds both spellings, and the `/` boundary keeps
/// `auth_plain.rb` from matching `not_auth_plain.rb`.
pub(crate) fn feature_already_loaded(path: &str) -> bool {
    let RubyValue::Array(features) = crate::globals::global_get(0, "$LOADED_FEATURES") else {
        return false;
    };
    let wanted = path.trim_end_matches(".rb");
    features.lock().iter().any(|f| {
        let RubyValue::Str(s) = f else { return false };
        let loaded = s.lock().to_utf8_lossy().into_owned();
        let loaded = loaded.trim_end_matches(".rb");
        loaded == wanted
            || loaded
                .strip_suffix(wanted)
                .is_some_and(|head| head.ends_with('/'))
    })
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

/// `Kernel#warn`: writes each message on its own line to `$stderr` and
/// returns nil. It does not double a trailing newline, the same rule `puts`
/// follows. The `uplevel:` keyword is not modelled, because kwargs never
/// reach the Kernel-function path.
pub fn kernel_warn(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    // A trailing keyword Hash (`category:`/`uplevel:`) is consumed, not printed.
    // CRuby leaves `Warning[:deprecated]` off by default (so a :deprecated
    // warning prints nothing), while :experimental and every other category
    // are on. The caller already evaluated the message arguments, so their
    // side effects happen regardless of suppression.
    let mut msgs = args;
    let mut uplevel: Option<usize> = None;
    if let Some(RubyValue::Hash(h)) = args.last() {
        let cat_key = RubyValue::Symbol(crate::Symbol::intern("category"));
        let up_key = RubyValue::Symbol(crate::Symbol::intern("uplevel"));
        let pairs = crate::hash_pairs(h);
        let is_kwargs = !pairs.is_empty()
            && pairs
                .iter()
                .all(|(k, _)| k.rb_eq(&cat_key) || k.rb_eq(&up_key));
        if is_kwargs {
            msgs = &args[..args.len() - 1];
            if let RubyValue::Symbol(s) = crate::hash_get(h, &cat_key)
                && s.name() == "deprecated"
            {
                return Ok(RubyValue::Nil);
            }
            if let RubyValue::Int(n) = crate::hash_get(h, &up_key)
                && n >= 0
            {
                uplevel = Some(n as usize);
            }
        }
    }
    let mut buf = Vec::new();
    // `uplevel: n` prefixes the first message with "file:line: warning: "
    // from the caller frame n levels up. This bare fn is folded into its
    // caller (no frame of its own), so the TOP frame is uplevel 0.
    if let Some(n) = uplevel
        && let Some(&(file, line, _)) = crate::frames::caller_frames(0).get(n)
    {
        buf.extend_from_slice(format!("{file}:{line}: warning: ").as_bytes());
    }
    // `rb_warn_m` renders its messages with `rb_io_puts` into a temp string,
    // so `warn` IS `puts` on stderr: an Array is one line per element
    // (recursively), an empty Array writes nothing, and a trailing newline is
    // never doubled. NO partial flush on a raising `to_s` -- CRuby renders
    // the whole message before its one write (unlike `puts`/`print`/`p`), so
    // nothing reaches stderr. And with no messages at all there is no write,
    // where bare `puts` would emit a newline.
    if msgs.is_empty() {
        return Ok(RubyValue::Nil);
    }
    crate::builtins::io::render_puts(msgs, &mut buf)?;
    crate::builtins::io::write_bytes(&crate::builtins::io::current_stderr(), &buf)?;
    Ok(RubyValue::Nil)
}

/// `Kernel#p`: each argument's INSPECT rendering on its own line; returns
/// nil / the single argument / the argument array (CRuby's exact shapes).
pub fn kernel_p(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    // CRuby's `rb_f_p` is a C frame: an `inspect` that raises shows
    // `in 'Kernel#p'` under the raising frame. Statically-emitted calls
    // bypass the dispatch boundary, so the frame is placed here.
    let _frame = crate::frames::synthetic_c_frame("Kernel#p");
    let mut buf = String::new();
    // Fallible: a raising user `inspect` propagates out of `p` (catchable,
    // CRuby's rule) -- after flushing the args already rendered, since
    // CRuby's rb_f_p prints per argument.
    let mut rendered = Ok(());
    for a in args {
        match a.try_inspect_string() {
            Ok(s) => {
                buf.push_str(&s);
                buf.push('\n');
            }
            Err(sig) => {
                rendered = Err(sig);
                break;
            }
        }
    }
    if !buf.is_empty() {
        crate::builtins::io::write_str(&crate::builtins::io::current_stdout(), &buf)?;
    }
    rendered?;
    Ok(match args.len() {
        0 => RubyValue::Nil,
        1 => args[0].clone(),
        _ => RubyValue::Array(crate::array_new(args.to_vec())),
    })
}

/// `Kernel#pp` -- for this runtime's value shapes, `p`'s rendering.
pub fn kernel_pp(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    kernel_p(args)
}

/// The raw bytes of an output-separator global (`$,`, `$\`), or `None` when it
/// is nil -- which is its default and the overwhelmingly common case, so the
/// caller adds nothing at all rather than an empty slice.
/// `$,` (the output FIELD separator) or `$\` (the output RECORD
/// separator) as raw bytes, or `None` when unset -- which is the default
/// for both. Ruby resolves them at the CALL, never at stream creation, so
/// every reader asks here.
pub(crate) fn output_separator(name: &str) -> Option<crate::enc::StrBuf> {
    match crate::globals::global_get(0, name) {
        RubyValue::Str(s) => Some(s.lock().clone()),
        _ => None,
    }
}

/// `Kernel#print`: display renderings joined by the output field separator
/// `$,` and closed by the output record separator `$\`, both nil (so both
/// empty) unless the program sets them. A String argument contributes its RAW
/// bytes (see `io::display_bytes`), which is what keeps `print 0xB4.chr` a
/// single byte on the fd.
pub fn kernel_print(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let mut buf = Vec::new();
    let mut rendered = Ok(());
    let field_sep = output_separator("$,");
    for (i, a) in args.iter().enumerate() {
        if i > 0
            && let Some(s) = &field_sep
        {
            buf.extend_from_slice(s.bytes());
        }
        if let Err(sig) = crate::builtins::io::display_bytes(a, &mut buf) {
            rendered = Err(sig);
            break;
        }
    }
    if rendered.is_ok()
        && let Some(s) = output_separator("$\\")
    {
        buf.extend_from_slice(s.bytes());
    }
    // Flush-then-propagate, same as `kernel_puts`.
    crate::builtins::io::write_bytes(&crate::builtins::io::current_stdout(), &buf)?;
    rendered?;
    Ok(RubyValue::Nil)
}

/// `Kernel#format`/`sprintf`.
pub fn kernel_format(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let Some((RubyValue::Str(template), rest)) = args.split_first() else {
        return Err(type_error!("no format string given"));
    };
    let template = template.lock().to_utf8_lossy().into_owned();
    Ok(RubyValue::Str(crate::string_new(
        crate::builtins::format::sprintf(&template, rest)?,
    )))
}

/// `Kernel#printf`.
pub fn kernel_printf(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if args.is_empty() {
        return Ok(RubyValue::Nil);
    }
    let formatted = kernel_format(args)?;
    crate::builtins::io::write_str(
        &crate::builtins::io::current_stdout(),
        &formatted.to_display_string(),
    )?;
    Ok(RubyValue::Nil)
}

/// The process-wide generator behind `rand`/`srand` -- MT19937, so a program
/// that `srand`s a fixed seed draws ruby's own sequence. Lazily seeded from
/// the clock on first use; `seed` is what `srand` reports back.
static PRNG: parking_lot::Mutex<Option<(crate::mt::Mt, u64)>> = parking_lot::Mutex::new(None);

/// The live generator, seeding it from the clock if nothing has yet.
fn with_prng<T>(f: impl FnOnce(&mut crate::mt::Mt) -> T) -> T {
    let mut guard = PRNG.lock();
    let entry = guard.get_or_insert_with(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e3779b97f4a7c15);
        (
            crate::mt::Mt::from_bigint(&num_bigint::BigInt::from(now)),
            now,
        )
    });
    f(&mut entry.0)
}

/// One uniform integer in `[0, limit]` from the process-wide generator.
pub(crate) fn prng_limited(limit: u64) -> u64 {
    with_prng(|mt| mt.limited(limit))
}

/// One uniform Float in `[0, 1)` from the process-wide generator.
pub(crate) fn prng_real() -> f64 {
    with_prng(crate::mt::Mt::next_real)
}

/// `Kernel#rand`: no arg -> Float in [0, 1); positive Integer n -> Integer
/// in [0, n); Float x -> Float in [0, x).
pub(crate) fn rand_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    Ok(match args.first() {
        None | Some(RubyValue::Nil) | Some(RubyValue::Int(0)) => RubyValue::Float(prng_real()),
        Some(RubyValue::Int(n)) if *n > 0 => RubyValue::Int(prng_limited(*n as u64 - 1) as i64),
        // A negative bound draws from `[0, |n|)` (a non-negative Integer).
        Some(RubyValue::Int(n)) => RubyValue::Int(prng_limited(n.unsigned_abs() - 1) as i64),
        // A bignum bound (`rand(2**70)`) draws from `[0, |n|)`: assemble enough
        // random words to cover the magnitude, then reduce mod |n|.
        Some(RubyValue::BigInt(n)) => {
            use num_bigint::{BigInt, Sign};
            let n: &BigInt = n;
            let magnitude = if n.sign() == Sign::Minus {
                -n
            } else {
                n.clone()
            };
            let _ = Sign::Plus;
            crate::builtins::integer::int_value(with_prng(|mt| mt.bigint_below(&magnitude)))
        }
        Some(RubyValue::Float(x)) => {
            // A non-finite bound has no Integer image: CRuby's `dbl2ival`
            // raises FloatDomainError named for the value ("Infinity"/"NaN").
            if !x.is_finite() {
                return Err(crate::dispatch::raise_error(
                    "FloatDomainError",
                    RubyValue::Float(*x).to_display_string(),
                ));
            }
            // CRuby's `Kernel#rand` truncates a Float bound to an Integer and
            // draws an Integer from `[0, ⌊x⌋)` (`rand(3.5)` -> 0..2). A bound
            // below 1 truncates to 0, i.e. the plain `[0.0, 1.0)` Float draw.
            let n = x.trunc();
            if n >= 1.0 {
                RubyValue::Int(prng_limited(n as u64 - 1) as i64)
            } else {
                RubyValue::Float(prng_real())
            }
        }
        Some(RubyValue::Range(__rg)) => {
            let (lo, hi, exclusive) = __rg.parts();
            return kernel_rand_range(lo, hi, exclusive);
        }
        Some(other) => {
            return Err(arg_error!(
                "invalid argument - {}",
                other.to_display_string()
            ));
        }
    })
}

/// `rand(a..b)` -- an Integer range yields an Integer, a Float endpoint yields
/// a Float. An empty/reversed range answers nil (CRuby's rule, NOT an error); a
/// beginless or endless range raises Errno::EDOM.
fn kernel_rand_range(
    lo: Option<&RubyValue>,
    hi: Option<&RubyValue>,
    exclusive: bool,
) -> Result<RubyValue, Signal> {
    let (Some(lo), Some(hi)) = (lo, hi) else {
        return Err(crate::dispatch::raise_error(
            "Errno::EDOM",
            "Numerical argument out of domain".to_string(),
        ));
    };
    match (lo, hi) {
        (RubyValue::Int(a), RubyValue::Int(b)) => {
            let span = b - a + i64::from(!exclusive);
            if span <= 0 {
                return Ok(RubyValue::Nil);
            }
            Ok(RubyValue::Int(a + prng_limited(span as u64 - 1) as i64))
        }
        _ => {
            let (Some(a), Some(b)) = (num_to_f64(lo), num_to_f64(hi)) else {
                // A Range whose endpoints aren't numeric: CRuby names the
                // Range in the generic to_int shape (oracle: `rand("a".."b")`
                // is "no implicit conversion of Range into Integer").
                return Err(type_error!("no implicit conversion of Range into Integer"));
            };
            if b < a || (b == a && exclusive) {
                return Ok(RubyValue::Nil);
            }
            // An inclusive range draws through a `[0, 1]` unit (see
            // `Random#rand`'s range arm).
            let unit = if exclusive {
                prng_real()
            } else {
                with_prng(crate::mt::Mt::next_real_inclusive)
            };
            Ok(RubyValue::Float(a + unit * (b - a)))
        }
    }
}

/// Integer/Float -> f64 (for a range endpoint); `None` otherwise.
fn num_to_f64(v: &RubyValue) -> Option<f64> {
    match v {
        RubyValue::Int(n) => Some(*n as f64),
        RubyValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// `Kernel#srand(seed)`: reseeds, returns the PREVIOUS seed.
pub(crate) fn srand_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let new_seed = match args.first() {
        Some(RubyValue::Int(n)) => *n as u64,
        _ => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1),
    };
    let mut guard = PRNG.lock();
    let previous = guard.as_ref().map_or(0, |(_, seed)| *seed);
    *guard = Some((
        crate::mt::Mt::from_bigint(&num_bigint::BigInt::from(new_seed)),
        new_seed,
    ));
    Ok(crate::builtins::integer::int_value(
        num_bigint::BigInt::from(previous),
    ))
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
        crate::check_ints()?;
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
        crate::gvl::process_gvl().without(|| ctx.sleep(remaining));
    }
    Ok(RubyValue::Int(
        started.elapsed().as_secs_f64().round() as i64
    ))
}

/// `Kernel#exit(status = true)` -- raises a RESCUABLE `SystemExit` carrying the
/// status, exactly as CRuby does: it unwinds through `ensure` blocks and can be
/// caught by `rescue SystemExit`. Only if it reaches the top level uncaught does
/// the process actually exit (see the generated `main`'s handler, which runs
/// `at_exit` first).
pub fn kernel_exit(args: &[RubyValue]) -> crate::Signal {
    let status = match args.first() {
        None | Some(RubyValue::Bool(true)) => 0,
        Some(RubyValue::Bool(false)) => 1,
        Some(RubyValue::Int(n)) => *n,
        Some(_) => 0,
    };
    crate::dispatch::raise_error_details(
        "SystemExit",
        "exit".to_string(),
        &[("status", RubyValue::Int(status))],
    )
}

/// `Kernel#abort(message = nil)` -- writes `message` to stderr IMMEDIATELY
/// (CRuby's own order, so it appears even when the SystemExit is rescued), then
/// raises `SystemExit` with status 1 and that message.
pub fn kernel_abort(args: &[RubyValue]) -> crate::Signal {
    let msg = match args.first() {
        Some(v) => {
            let s = v.to_display_string();
            eprintln!("{s}");
            s
        }
        None => "exit".to_string(),
    };
    crate::dispatch::raise_error_details("SystemExit", msg, &[("status", RubyValue::Int(1))])
}

/// `Kernel#exit!(status = false)` -- CRuby's uncatchable immediate exit: no
/// `SystemExit`, no `ensure`, no `at_exit`.
pub fn kernel_exit_bang(args: &[RubyValue]) -> ! {
    let code = match args.first() {
        None | Some(RubyValue::Bool(false)) => 1,
        Some(RubyValue::Bool(true)) => 0,
        Some(RubyValue::Int(n)) => *n as i32,
        Some(_) => 1,
    };
    // CRuby's `exit!` is `_exit(2)`: it runs no `at_exit` handler and
    // DISCARDS whatever stdio still holds (`print "x"; exit!` writes
    // nothing, where `exit` writes the `x`). `std::process::exit` runs
    // Rust's own cleanup, which flushes -- so the buffer has to be
    // stepped around, not asked politely.
    unsafe { libc::_exit(code) }
}

/// The exit status carried by `exc` when it IS a `SystemExit`, else `None` --
/// what the generated top-level consults to exit quietly with that status
/// instead of reporting an uncaught exception.
pub fn system_exit_status(exc: &RubyValue) -> Option<i32> {
    let RubyValue::Object(o) = exc else {
        return None;
    };
    if !crate::dispatch::is_a(o.class_id(), zeo_abi::SYSTEM_EXIT_CLASS) {
        return None;
    }
    // Read the status through its own `#status` row rather than a private
    // detail accessor, so the two can't drift.
    Some(
        match crate::dispatch::send_value(exc, crate::Symbol::intern("status"), &[], None) {
            Ok(RubyValue::Int(n)) => n as i32,
            _ => 0,
        },
    )
}

/// The exception `raise`'s argument list names -- everything the row does
/// before it signals. Shared with the explicit-`cause:` entry, which needs
/// the built exception in hand before it raises.
pub(crate) fn build_raise_exception(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match args {
        // Bare re-raise: the exception being rescued, exactly (same
        // object); outside any rescue, a fresh EMPTY-message
        // RuntimeError (oracle-verified, matching `emit_raise`).
        [] => match crate::current_exception() {
            Some(e) => Ok(e),
            None => {
                crate::dispatch::coerce_raise_arg(RubyValue::Str(crate::string_new(String::new())))
            }
        },
        [first, rest @ ..] => {
            // At most the message reaches `exception`; `rest[1]` is the
            // dropped backtrace.
            let msg = &rest[..rest.len().min(1)];
            match first {
                RubyValue::Class(cid) => {
                    if !crate::dispatch::is_a(*cid, zeo_abi::EXCEPTION_CLASS) {
                        return Err(type_error!("exception class/object expected"));
                    }
                    crate::dispatch::send_value(first, Symbol::intern("exception"), msg, None)
                }
                RubyValue::Object(o)
                    if crate::dispatch::is_a(o.class_id(), zeo_abi::EXCEPTION_CLASS) =>
                {
                    if msg.is_empty() {
                        Ok(first.clone())
                    } else {
                        crate::dispatch::send_value(first, Symbol::intern("exception"), msg, None)
                    }
                }
                RubyValue::Str(_) if rest.is_empty() => {
                    crate::dispatch::coerce_raise_arg(first.clone())
                }
                // Anything else goes through CRuby's `rb_make_exception`
                // protocol: an object whose class answers `#exception` may
                // be raised, and only what that refuses is the TypeError.
                // Refusing here outright made `raise WithHook.new, "hi"`
                // a TypeError where the emitter's own folded site ran the
                // hook.
                _ if msg.is_empty() => crate::dispatch::coerce_raise_arg(first.clone()),
                _ => crate::dispatch::coerce_raise_arg_with_message(first.clone(), msg[0].clone()),
            }
        }
    }
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

    #[test]
    fn eql_requires_same_class_and_equality() {
        let t = imethod("eql?")(&RubyValue::Int(1), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(t, RubyValue::Bool(true)));
        let f = imethod("eql?")(&RubyValue::Int(1), &[RubyValue::Float(1.0)], None).unwrap();
        assert!(matches!(f, RubyValue::Bool(false)));
    }

    #[test]
    fn to_s_and_inspect_render_like_puts_and_p() {
        let s = imethod("to_s")(&RubyValue::Nil, &[], None).unwrap();
        let RubyValue::Str(s) = s else { panic!() };
        assert_eq!(&*s.lock().to_utf8_lossy(), "");
        let i = imethod("inspect")(&RubyValue::Nil, &[], None).unwrap();
        let RubyValue::Str(i) = i else { panic!() };
        assert_eq!(&*i.lock().to_utf8_lossy(), "nil");
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
