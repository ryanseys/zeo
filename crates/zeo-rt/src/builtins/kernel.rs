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

use crate::builtins::{
    arg_error, arity, block_or_enum, builtin_methods, local_jump_error, need_block, not_impl_error,
    type_error,
};
use crate::{RubyValue, Signal, Symbol};

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

builtin_methods! {
    pub(crate) fn lookup;

    // `send`/`public_send` are KERNEL's, not BasicObject's (vm_eval.c:2961,
    // :2963) -- which is what makes them absent on a blank-slate receiver
    // while `__send__` still works there. They share BasicObject's one
    // implementation, as `rb_f_send` does in CRuby.
    //
    // `public_send`'s visibility gate lives in `dispatch::send_value_public_in`
    // and is applied by codegen at the call site, so this row is the
    // visibility-blind path both names funnel through once that check passes.
    "send" | "public_send" => fn kernel_send(recv, args, block) {
        crate::builtins::basic_object::dynamic_send(recv, args, block)
    }

    // The print family as REAL Kernel methods (Path 2): `obj.send(:puts,
    // ...)`, `self.puts` on `main`, and any dynamic dispatch reach these;
    // the receiver is ignored, exactly like CRuby's private Kernel#puts.
    "puts" => fn puts(_recv, args, _block) {
        kernel_puts(args)
    }
    "print" => fn print(_recv, args, _block) {
        kernel_print(args)
    }
    "p" => fn p(_recv, args, _block) {
        kernel_p(args)
    }
    // `Kernel#open(path, mode = "r")` -- opens a File (the `"|command"` pipe
    // form is out of scope); delegates to `File.open` so the block-closes-file
    // contract and mode handling are shared, never divergent.
    "open" => fn kernel_open(_recv, args, block) {
        crate::builtins::file::lookup_class("open").unwrap()(
            &RubyValue::Class(zeo_abi::FILE_CLASS),
            args,
            block,
        )
    }
    "pp" => fn pp(_recv, args, _block) {
        kernel_pp(args)
    }
    "warn" => fn warn(_recv, args, _block) {
        kernel_warn(args)
    }
    // Spawning a child. `system` inherits stdout/stderr and answers a
    // true/false/nil verdict; the backtick captures stdout and answers it as a
    // String. Both set `$?` (see `builtins::process`). Private Kernel methods,
    // so `respond_to?`'s default hides them (see `is_hidden_builtin_private`).
    "system" => fn system(recv, args, block) {
        crate::builtins::process::system(recv, args, block)
    }
    "`" => fn backquote(recv, args, block) {
        crate::builtins::process::backquote(recv, args, block)
    }
    // `putc` -- writes one character to `$stdout` and returns its argument.
    // An Integer writes the low byte (`n & 0xff`); a String writes its first
    // character.
    "putc" => fn putc(_recv, args, _block) {
        arity!(args, 1);
        let out = crate::builtins::io::current_stdout();
        match &args[0] {
            RubyValue::Int(n) => {
                let byte = (n & 0xff) as u8;
                crate::builtins::io::write_str(&out, &(byte as char).to_string())?;
            }
            RubyValue::Str(s) => {
                let text = s.lock().to_utf8_lossy().into_owned();
                if let Some(c) = text.chars().next() {
                    crate::builtins::io::write_str(&out, &c.to_string())?;
                }
            }
            // NUM2CHR: the low byte of the `to_int` conversion (`putc 2.5`
            // truncates; no `to_str` duck here -- oracle-verified).
            other => {
                let byte = (crate::builtins::convert::to_index(other)? & 0xff) as u8;
                crate::builtins::io::write_str(&out, &(byte as char).to_string())?;
            }
        }
        Ok(args[0].clone())
    }
    // `public_method(:name)` -- a bound Method restricted to the public
    // surface (a private/protected name raises NameError).
    "public_method" => fn public_method(recv, args, _block) {
        arity!(args, 1);
        crate::builtins::method_obj::public_method_new(recv, &args[0])
    }
    // `Kernel#method(:name)` -- a bound Method object (see
    // `builtins::method_obj`). Reaches every receiver via the MRO walk's
    // Kernel row, including the top-level `main` object.
    "method" => fn method(recv, args, _block) {
        arity!(args, 1);
        crate::builtins::method_obj::method_new(recv, &args[0])
    }
    "singleton_method" => fn singleton_method(recv, args, _block) {
        arity!(args, 1);
        crate::builtins::method_obj::singleton_method_new(recv, &args[0])
    }
    // `obj.singleton_class` -- the per-object singleton class as a real Class
    // value; defining a method on it installs a per-object singleton (see
    // `runtime_meta::runtime_singleton_class`).
    "singleton_class" => fn singleton_class(recv, args, _block) {
        arity!(args, 0);
        crate::runtime_meta::runtime_singleton_class(recv)
    }
    // `obj.extend(Mod, ...)` -- mix each module's instance methods into the
    // receiver's singleton. The bare `extend Mod` STATEMENT form (no receiver)
    // is a separate parse-level mixin; this row is the method-call form only.
    "extend" => fn extend_obj(recv, args, _block) {
        if args.is_empty() {
            return Err(arg_error!("wrong number of arguments (given 0, expected 1+)"));
        }
        for m in args {
            crate::runtime_meta::runtime_extend(recv, m)?;
        }
        Ok(recv.clone())
    }
    // `Object#define_singleton_method(name) { body }` (#97) -- a per-object
    // singleton on an ordinary receiver, or a class/singleton method when the
    // receiver is a `Class`. Universal (this Kernel row is reached by every
    // receiver's MRO walk, including a class value). A singleton on an
    // immediate (Integer/Symbol/nil/...) is a `TypeError`, like CRuby.
    "define_singleton_method" => fn define_singleton_method(recv, args, block) {
        arity!(args, 1..=2);
        let name = crate::runtime_meta::coerce_method_name(args.first())?;
        let body = crate::runtime_meta::coerce_method_body(args, &block)?;
        crate::runtime_define_singleton_method(recv, name, body)
    }
    // `eval(str)` (#97 stage 2) -- runtime string eval through the eval VM
    // (feature-gated: a build without `eval-vm` answers NotImplementedError).
    // `self` is the CALLER's own, since this universal Kernel row is reached
    // through the receiver's MRO walk -- so `eval("@x")` at the top level reads
    // the main object's ivar, and the same call inside a method reads that
    // receiver's. The binding/filename/lineno arguments are the next
    // increment: an explicit non-nil binding is a clean NotImplementedError,
    // filename/lineno are accepted and ignored.
    "eval" => fn eval(recv, args, _block) {
        arity!(args, 1..=4);
        if let Some(binding) = args.get(1) {
            if !binding.is_nil() {
                return Err(not_impl_error!("eval with an explicit binding is not supported yet"));
            }
        }
        crate::eval_value(args[0].clone(), recv.clone(), 0)
    }
    // `catch(tag = new object) { |tag| ... }` / `throw(tag[, value])` /
    // `sleep(secs)` -- universal Kernel methods. The static codegen fast path
    // handles the literal `catch {}`/`throw` forms; these rows serve dynamic
    // dispatch (a `send :catch`, a `catch` reached through the MRO walk).
    "catch" => fn catch_m(_recv, args, block) {
        arity!(args, 0..=1);
        // A bare `catch` mints a fresh, unique tag object (passed to the block).
        let tag = args
            .first()
            .cloned()
            .unwrap_or_else(|| RubyValue::Array(crate::array_new(Vec::new())));
        let blk = block.ok_or_else(|| {
            local_jump_error!("no block given (yield)")
        })?;
        crate::kernel_catch(tag, blk)
    }
    "throw" => fn throw_m(_recv, args, _block) {
        crate::kernel_throw(args)
    }
    "sleep" => fn sleep_m(_recv, args, _block) {
        crate::kernel_sleep(args)
    }
    "class" => fn class(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Class(recv.class_id()))
    }
    // `object_id` -- a stable per-identity Integer. Objects use their `Arc`
    // pointer; immediates use CRuby's fixed/derived shapes (Integers
    // `2n+1`, nil/true/false their reserved slots). Strings/Arrays/Hashes
    // use their cell pointer -- identity, not content.
    "object_id" | "__id__" => fn object_id(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(match recv {
            RubyValue::Int(i) => i.wrapping_mul(2).wrapping_add(1),
            // CRuby 4.0.5's fixed immediate ids: nil 4, true 20, false 0.
            RubyValue::Nil => 4,
            RubyValue::Bool(true) => 20,
            RubyValue::Bool(false) => 0,
            RubyValue::Object(o) => std::sync::Arc::as_ptr(o) as *const () as i64,
            RubyValue::Str(s) => std::sync::Arc::as_ptr(s) as i64,
            RubyValue::Array(a) => std::sync::Arc::as_ptr(a) as i64,
            RubyValue::Hash(h) => std::sync::Arc::as_ptr(h) as i64,
            RubyValue::Symbol(s) => 0x1000_0000_0000 + i64::from(s.to_u32()),
            // The remaining kinds get a per-call address-ish value -- a
            // documented approximation (identity comparison via object_id
            // on them is rare).
            _ => recv as *const _ as i64,
        }))
    }
    "nil?"[0] => fn nil_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv.is_nil()))
    }
    "itself" => fn itself(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "caller" => fn caller_m(_recv, args, _block) {
        // AOT builds keep no runtime call-stack frames, so `caller` is always an
        // empty Array (never `nil`). The optional `(start, length)` / Range
        // window is honored trivially -- every window over zero frames is empty.
        // The arguments are still accepted (and were already evaluated for their
        // side effects at the call site), matching CRuby's contract shape.
        arity!(args, 0..=2);
        Ok(RubyValue::Array(crate::array_new(Vec::new())))
    }
    "caller_locations" => fn caller_locations_m(_recv, args, _block) {
        // Same no-frames reality as `caller`: an empty Array (not `nil`), so the
        // `caller_locations(..)&.first` guard idiom and `.each`/`.map` iteration
        // stay safe.
        arity!(args, 0..=2);
        Ok(RubyValue::Array(crate::array_new(Vec::new())))
    }
    // The private `Kernel` conversion and formatting functions, as real methods
    // so they resolve through EVERY dispatch path -- a splat call (`format(*a)`),
    // `method(:Integer)`, `send`, a curry -- not only the codegen fast-path that
    // intercepts a direct literal call. Each delegates to the same runtime
    // routine that fast-path emits, so behavior is identical however it's reached.
    "format" | "sprintf" => fn format_m(_recv, args, _block) {
        kernel_format(args)
    }
    "Integer" => fn integer_m(_recv, args, _block) {
        kernel_integer(args)
    }
    "Float" => fn float_m(_recv, args, _block) {
        kernel_float(args)
    }
    "String" => fn string_conv_m(_recv, args, _block) {
        kernel_string(args)
    }
    "Array" => fn array_conv_m(_recv, args, _block) {
        kernel_array(args)
    }
    "Hash" => fn hash_conv_m(_recv, args, _block) {
        kernel_hash(args)
    }
    "Rational" => fn rational_m(_recv, args, _block) {
        kernel_rational(args)
    }
    "Complex" => fn complex_m(_recv, args, _block) {
        kernel_complex(args)
    }
    // Private `Kernel#trap` -- the receiverless spelling of `Signal.trap`, same
    // validated no-op that records the action and returns the prior one.
    "trap" => fn trap_m(_recv, args, block) {
        crate::builtins::signal::trap_impl(args, block)
    }
    // `proc(&b)` / `proc { }` -- answer the passed block as a Proc (it already IS
    // one at the ABI level). No block is CRuby's `ArgumentError`.
    "proc" => fn proc_m(_recv, args, block) {
        arity!(args, 0);
        match block {
            Some(b @ RubyValue::Proc(_)) => Ok(b),
            _ => Err(arg_error!("tried to create Proc object without a block")),
        }
    }
    "dup" => fn dup(recv, args, _block) {
        arity!(args, 0);
        Ok(match recv {
            RubyValue::Object(o) => copy_with_hook(recv, RubyValue::Object(o.dup_object(false)))?,
            _ => recv.dup_value(false),
        })
    }
    "clone" => fn clone_m(recv, args, _block) {
        arity!(args, 0..=1);
        // `clone(freeze: nil)` PRESERVES the original's frozen state (the
        // default), `freeze: true` forces the copy frozen, `freeze: false`
        // forces it unfrozen. The keyword arrives as a trailing options Hash.
        let freeze = match args.first() {
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
            RubyValue::Object(o) => copy_with_hook(recv, RubyValue::Object(o.dup_object(copy_frozen)))?,
            _ => recv.dup_value(copy_frozen),
        };
        if freeze == Some(true) {
            copy.freeze_value();
        }
        Ok(copy)
    }
    "frozen?" => fn frozen_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv.is_frozen()))
    }
    "freeze" => fn freeze(recv, args, _block) {
        arity!(args, 0);
        recv.freeze_value();
        Ok(recv.clone())
    }
    "hash" => fn hash(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(crate::value_hash_code(recv)))
    }
    "to_s"[0] => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(recv.to_display_string())))
    }
    "inspect"[0] => fn inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(recv.inspect_string())))
    }
    // Kernel's default `===` is `==` (case subjects fall back to equality).
    "===" => fn case_eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    // The default `Object#<=>`: `0` when the two are `==`, else `nil` (no
    // ordering). Classes with a real ordering (Integer/String/Array/... )
    // define their own `<=>`, which the MRO walk reaches before this Kernel
    // fallback, so this only answers for the un-ordered types (Hash, Range,
    // Regexp, nil, true/false, Proc, Complex).
    "<=>" => fn spaceship(recv, args, _block) {
        arity!(args, 1);
        Ok(if recv.rb_eq(&args[0]) {
            RubyValue::Int(0)
        } else {
            RubyValue::Nil
        })
    }
    // `eql?`: same class AND `==` (what makes `1.eql?(1.0)` false while
    // `1 == 1.0` is true -- oracle-verified).
    "eql?" => fn eql_p(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(
            recv.class_id() == args[0].class_id() && recv.rb_eq(&args[0]),
        ))
    }
    "is_a?" | "kind_of?" => fn is_a_p(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Class(target) = &args[0] else {
            return Err(type_error!("class or module required"));
        };
        Ok(RubyValue::Bool(crate::dispatch::is_a(recv.class_id(), *target)))
    }
    "instance_of?" => fn instance_of_p(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Class(target) = &args[0] else {
            return Err(type_error!("class or module required"));
        };
        Ok(RubyValue::Bool(recv.class_id() == *target))
    }
    // `respond_to?(name, include_all = false)` -- the second parameter
    // opts private methods back in (CRuby's default ignores them).
    "respond_to?" => fn respond_to_p(recv, args, _block) {
        arity!(args, 1..=2);
        let sym = match &args[0] {
            RubyValue::Symbol(s) => *s,
            RubyValue::Str(s) => Symbol::intern(&s.lock().to_utf8_lossy()),
            other => {
                return Err(type_error!("{} is not a symbol nor a string", other.inspect_string()))
            }
        };
        let include_all = args.get(1).is_some_and(|v| v.truthy());
        // A per-object singleton method (#97 F3) answers first -- it's keyed by
        // object identity, invisible to the class-ancestry walk below.
        if crate::runtime_meta::is_live()
            && crate::runtime_meta::object_has_singleton_method(recv, sym)
        {
            return Ok(RubyValue::Bool(true));
        }
        Ok(RubyValue::Bool(crate::dispatch::responds_to(recv.class_id(), sym, include_all)))
    }
    // Universal named-ivar reflection over ANY receiver (an `Object`'s or a
    // class object's ivars; a builtin/immediate exposes none). Registering
    // these on Kernel is also what makes `respond_to?(:instance_variable_get)`
    // and a dynamic `send(:instance_variables)` resolve them uniformly -- the
    // static codegen path (call.rs) is just a fast path over the same helpers.
    "instance_variables" => fn instance_variables_m(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::dispatch::instance_variables(recv))
    }
    "instance_variable_get" => fn instance_variable_get_m(recv, args, _block) {
        arity!(args, 1);
        crate::dispatch::instance_variable_get(recv, &args[0])
    }
    "instance_variable_set" => fn instance_variable_set_m(recv, args, _block) {
        arity!(args, 2);
        crate::dispatch::instance_variable_set(recv, &args[0], args[1].clone())
    }
    "remove_instance_variable" => fn remove_instance_variable_m(recv, args, _block) {
        arity!(args, 1);
        crate::dispatch::remove_instance_variable(recv, &args[0])
    }
    "instance_variable_defined?" => fn instance_variable_defined_m(recv, args, _block) {
        arity!(args, 1);
        let name = crate::dispatch::ivar_name_arg(&args[0])?;
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
    "methods" | "public_methods" => fn methods_m(recv, args, _block) {
        arity!(args, 0..=1);
        let inherit = !matches!(args.first(), Some(RubyValue::Bool(false)) | Some(RubyValue::Nil));
        let mut names = Vec::new();
        if let RubyValue::Class(cid) = recv {
            names.extend(crate::dispatch::class_method_names(*cid));
        }
        // `Object#methods` returns public AND protected names.
        names.extend(crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::VisFilter::NotPrivate,
            inherit,
        ));
        Ok(syms_to_array(dedup_syms(names)))
    }
    "private_methods" => fn private_methods_m(recv, args, _block) {
        arity!(args, 0..=1);
        let inherit = !matches!(args.first(), Some(RubyValue::Bool(false)) | Some(RubyValue::Nil));
        let names = crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::VisFilter::Private,
            inherit,
        );
        Ok(syms_to_array(names))
    }
    "protected_methods" => fn protected_methods_m(recv, args, _block) {
        arity!(args, 0..=1);
        let inherit = !matches!(args.first(), Some(RubyValue::Bool(false)) | Some(RubyValue::Nil));
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
    "singleton_methods" => fn singleton_methods_m(recv, args, _block) {
        arity!(args, 0..=1);
        let names = match recv {
            RubyValue::Class(cid) => crate::dispatch::class_method_names(*cid),
            _ => Vec::new(),
        };
        Ok(syms_to_array(names))
    }
    // `Object#display([port])` -- writes `self.to_s` (no newline) to stdout
    // and answers nil. The optional port argument is accepted but ignored
    // (only the process stdout is modeled).
    "display" => fn display(recv, args, _block) {
        arity!(args, 0..=1);
        kernel_print(std::slice::from_ref(recv))
    }
    // `Object#!~` -- the negation of `=~`, dispatched to the receiver's own
    // `=~` (so a receiver without one raises NoMethodError, exactly as CRuby
    // does since `Object#=~` was removed).
    "!~" => fn not_match(recv, args, _block) {
        arity!(args, 1);
        let matched = crate::dispatch::send_value(recv, crate::Symbol::intern("=~"), args, None)?;
        Ok(RubyValue::Bool(!matched.truthy()))
    }
    "tap" => fn tap(recv, args, block) {
        arity!(args, 0);
        let p = need_block!(block);
        p.call(std::slice::from_ref(recv))?;
        Ok(recv.clone())
    }
    "then" | "yield_self" => fn then_m(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "then", args, block);
        p.call(std::slice::from_ref(recv))
    }
    // `x.to_enum(:meth, *args)` -- captures exactly (receiver, method,
    // args), CRuby's obj_to_enum. The block-as-size-proc
    // form is Tier B (rare; the stored-size Enumerator.new form covers
    // the practical cases).
    "to_enum" | "enum_for" => fn to_enum(recv, args, _block) {
        let meth = match args.first() {
            None => "each".to_string(),
            Some(RubyValue::Symbol(s)) => s.name().as_str().to_string(),
            Some(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
            Some(other) => {
                return Err(type_error!("{} is not a symbol nor a string", other.inspect_string()))
            }
        };
        let rest = if args.is_empty() { &[] } else { &args[1..] };
        Ok(crate::builtins::enumerator::enumerator_for(recv, &meth, rest))
    }
}

/// Run the (user-overridable) `initialize_copy` hook on a freshly
/// shallow-copied object, with the original as its argument -- real Ruby's
/// `clone`/`dup` contract. Object's default hook is a no-op; a user
/// override (e.g. deep-copying a shared member) runs here.
fn copy_with_hook(original: &RubyValue, copy: RubyValue) -> Result<RubyValue, Signal> {
    let hook = Symbol::intern("initialize_copy");
    // Only dispatch when the object actually defines the (private) hook --
    // never let a missing one fall through to `method_missing`. In a real
    // program Object's default no-op makes this always true; a user override
    // runs here.
    if crate::dispatch::responds_to(copy.class_id(), hook, true) {
        crate::dispatch::send_value(&copy, hook, std::slice::from_ref(original), None)?;
    }
    Ok(copy)
}

/// `Kernel#Integer(arg, base = nil)` -- CRuby's strict conversion: strings
/// allow surrounding whitespace, single underscores between digits, and
/// radix prefixes (`0x`/`0o`/`0b`, or a leading `0` octal when no base is
/// given); floats/rationals TRUNCATE toward zero; nil and everything else
/// is a TypeError. Message shapes oracle-verified.
pub fn kernel_integer(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    let base = match args.get(1) {
        None => None,
        Some(v) => Some(crate::builtins::convert::to_index(v)? as u32),
    };
    // A base only makes sense for a String argument -- CRuby raises rather than
    // silently ignoring it for an Integer/Float/etc. (#2515).
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
    } else if lower.len() > 1 && lower.starts_with('0') && base.is_none() {
        (8, lower[1..].to_string())
    } else {
        (base.unwrap_or(10), lower)
    };
    if let Some(b) = base {
        if b != radix && !(b == 10 && radix == 10) {
            // An explicit base must agree with an explicit prefix.
            if radix != b {
                return None;
            }
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
pub fn kernel_float(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    match &args[0] {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
            Ok(RubyValue::Float(
                crate::builtins::numeric::num_to_f64_unchecked(&args[0]),
            ))
        }
        RubyValue::Str(s) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            let clean: String = text.trim().chars().filter(|c| *c != '_').collect();
            clean
                .parse::<f64>()
                .ok()
                .filter(|f| f.is_finite() || clean.to_ascii_lowercase().contains("inf"))
                // C99 hex-float (`"0x1p4"` = 16.0), which `str::parse` rejects.
                .or_else(|| parse_hex_float(&clean))
                .map(RubyValue::Float)
                .ok_or_else(|| arg_error!("invalid value for Float(): {:?}", text))
        }
        other => Err(type_error!(
            "can't convert {} into Float",
            crate::builtins::convert_name_of(other)
        )),
    }
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
pub fn kernel_rational(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
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
pub fn kernel_complex(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    if let (RubyValue::Str(s), None) = (&args[0], args.get(1)) {
        let (real, imag) = parse_complex_string(&s.lock().to_utf8_lossy())?;
        return crate::builtins::complex::complex_new(real, imag);
    }
    let imag = args.get(1).cloned().unwrap_or(RubyValue::Int(0));
    crate::builtins::complex::complex_new(args[0].clone(), imag)
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
pub fn kernel_string(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    Ok(RubyValue::Str(crate::string_new(
        args[0].to_display_string(),
    )))
}

/// `Kernel#Array(arg)`: nil -> [], Array -> itself, Hash -> assoc pairs,
/// Range -> to_a, anything else -> [arg]. (`to_ary`/`to_a` protocol probes
/// on user objects are a documented scope-cut.)
pub fn kernel_array(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
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
pub fn kernel_hash(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    match &args[0] {
        RubyValue::Nil => Ok(RubyValue::Hash(crate::hash_new(Vec::new()))),
        RubyValue::Array(a) if a.lock().is_empty() => {
            Ok(RubyValue::Hash(crate::hash_new(Vec::new())))
        }
        RubyValue::Hash(_) => Ok(args[0].clone()),
        other => Err(type_error!(
            "can't convert {} into Hash",
            crate::builtins::convert_name_of(other)
        )),
    }
}

/// `Kernel#puts`: zero args print one newline; arrays flatten recursively,
/// each scalar on its own line (nil renders empty) -- CRuby's exact rules.
/// Routed through whatever `$stdout` currently holds (default: the
/// `STDOUT` singleton) -- `$stdout = STDERR` or any duck-typed writer
/// redirects the whole print family. See `builtins::io` for the rendering
/// and write plumbing.
pub fn kernel_puts(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let mut buf = String::new();
    crate::builtins::io::render_puts(args, &mut buf);
    crate::builtins::io::write_str(&crate::builtins::io::current_stdout(), &buf)?;
    Ok(RubyValue::Nil)
}

/// `Kernel#warn`: each message on its own line (no trailing newline
/// doubling, same rule as `puts`) to `$stderr`; returns nil. The
/// `uplevel:` keyword isn't modeled (kwargs never reach the
/// Kernel-function path).
pub fn kernel_warn(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    // A trailing keyword Hash (`category:`/`uplevel:`) is consumed, not printed.
    // CRuby leaves `Warning[:deprecated]` off by default (so a :deprecated
    // warning prints nothing), while :experimental and every other category
    // are on. The caller already evaluated the message arguments, so their
    // side effects happen regardless of suppression.
    let mut msgs = args;
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
            if let RubyValue::Symbol(s) = crate::hash_get(h, &cat_key) {
                if s.name() == "deprecated" {
                    return Ok(RubyValue::Nil);
                }
            }
        }
    }
    let mut buf = String::new();
    for a in msgs {
        let s = a.to_display_string();
        buf.push_str(&s);
        if !s.ends_with('\n') {
            buf.push('\n');
        }
    }
    crate::builtins::io::write_str(&crate::builtins::io::current_stderr(), &buf)?;
    Ok(RubyValue::Nil)
}

/// `Kernel#p`: each argument's INSPECT rendering on its own line; returns
/// nil / the single argument / the argument array (CRuby's exact shapes).
pub fn kernel_p(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let mut buf = String::new();
    for a in args {
        buf.push_str(&a.inspect_string());
        buf.push('\n');
    }
    if !buf.is_empty() {
        crate::builtins::io::write_str(&crate::builtins::io::current_stdout(), &buf)?;
    }
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

/// `Kernel#print`: display renderings, no separators, no newline.
pub fn kernel_print(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let mut buf = String::new();
    for a in args {
        buf.push_str(&a.to_display_string());
    }
    crate::builtins::io::write_str(&crate::builtins::io::current_stdout(), &buf)?;
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

/// The process-wide PRNG behind `rand`/`srand` -- xorshift64*, reseedable.
/// DOCUMENTED DIVERGENCE: not CRuby's MT19937, so seeded SEQUENCES differ;
/// oracle tests assert ranges/properties, never exact values.
static PRNG: parking_lot::Mutex<(u64, u64)> = parking_lot::Mutex::new((0, 0)); // (state, seed)

pub(crate) fn prng_next() -> u64 {
    let mut guard = PRNG.lock();
    if guard.0 == 0 {
        // First use, unseeded: derive from the clock.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e3779b97f4a7c15);
        *guard = (now | 1, now);
    }
    let mut x = guard.0;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    guard.0 = x;
    x.wrapping_mul(0x2545f4914f6cdd1d)
}

/// `Kernel#rand`: no arg -> Float in [0, 1); positive Integer n -> Integer
/// in [0, n); Float x -> Float in [0, x).
pub fn kernel_rand(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0..=1);
    let r = prng_next();
    Ok(match args.first() {
        None | Some(RubyValue::Nil) | Some(RubyValue::Int(0)) => {
            RubyValue::Float((r >> 11) as f64 / (1u64 << 53) as f64)
        }
        Some(RubyValue::Int(n)) if *n > 0 => RubyValue::Int((r % (*n as u64)) as i64),
        // A negative bound draws from `[0, |n|)` (a non-negative Integer).
        Some(RubyValue::Int(n)) => RubyValue::Int((r % n.unsigned_abs()) as i64),
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
            let words = (magnitude.bits() / 64 + 1) as usize;
            let mut bytes = r.to_le_bytes().to_vec();
            for _ in 1..words {
                bytes.extend_from_slice(&prng_next().to_le_bytes());
            }
            crate::builtins::integer::int_value(
                BigInt::from_bytes_le(Sign::Plus, &bytes) % magnitude,
            )
        }
        Some(RubyValue::Float(x)) => {
            // CRuby's `Kernel#rand` truncates a Float bound to an Integer and
            // draws an Integer from `[0, ⌊x⌋)` (`rand(3.5)` -> 0..2). A bound
            // below 1 truncates to 0, i.e. the plain `[0.0, 1.0)` Float draw.
            let n = x.trunc();
            if n >= 1.0 {
                RubyValue::Int((r % (n as u64)) as i64)
            } else {
                RubyValue::Float((r >> 11) as f64 / (1u64 << 53) as f64)
            }
        }
        Some(RubyValue::Range(lo, hi, exclusive)) => {
            return kernel_rand_range(r, lo.as_deref(), hi.as_deref(), *exclusive);
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
    r: u64,
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
    let unit = (r >> 11) as f64 / (1u64 << 53) as f64;
    match (lo, hi) {
        (RubyValue::Int(a), RubyValue::Int(b)) => {
            let span = b - a + i64::from(!exclusive);
            if span <= 0 {
                return Ok(RubyValue::Nil);
            }
            Ok(RubyValue::Int(a + (r % span as u64) as i64))
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
pub fn kernel_srand(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0..=1);
    let new_seed = match args.first() {
        Some(RubyValue::Int(n)) => *n as u64,
        _ => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1),
    };
    let mut guard = PRNG.lock();
    let previous = guard.1;
    *guard = (new_seed | 1, new_seed);
    Ok(crate::builtins::integer::int_value(
        num_bigint::BigInt::from(previous),
    ))
}

// The per-coroutine stack of tags with a live `catch` frame. `throw` consults
// it so an unmatched tag becomes an `UncaughtThrowError` AT THE THROW (as in
// CRuby), rather than a `Signal::Throw` leaking past every `rescue` to the top
// level. Coroutine-local: each `Thread`/`Fiber` unwinds its own catch frames.
may::coroutine_local!(static CATCH_TAGS: std::cell::RefCell<Vec<RubyValue>> = std::cell::RefCell::new(Vec::new()));

/// `Kernel#catch(tag) { ... }` / `Kernel#throw(tag[, value])`.
pub fn kernel_catch(tag: RubyValue, block: RubyValue) -> Result<RubyValue, Signal> {
    let RubyValue::Proc(p) = &block else {
        panic!("Kernel#catch requires a block");
    };
    CATCH_TAGS.with(|s| s.borrow_mut().push(tag.clone()));
    let result = p.call(std::slice::from_ref(&tag));
    CATCH_TAGS.with(|s| {
        s.borrow_mut().pop();
    });
    match result {
        Err(Signal::Throw(t, v)) if t.rb_eq(&tag) => Ok(v),
        other => other,
    }
}

pub fn kernel_throw(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    let tag = args[0].clone();
    // Only a tag with a live `catch` frame may unwind; otherwise it is an
    // `UncaughtThrowError` right here, catchable by an ordinary `rescue`.
    let has_live_catch = CATCH_TAGS.with(|s| s.borrow().iter().any(|t| t.rb_eq(&tag)));
    if has_live_catch {
        Err(Signal::Throw(
            tag,
            args.get(1).cloned().unwrap_or(RubyValue::Nil),
        ))
    } else {
        Err(crate::dispatch::raise_error_details(
            "UncaughtThrowError",
            format!("uncaught throw {}", tag.inspect_string()),
            &[
                ("tag", tag.clone()),
                ("value", args.get(1).cloned().unwrap_or(RubyValue::Nil)),
            ],
        ))
    }
}

/// `Kernel#sleep(seconds)` -- cooperative (`may`'s coroutine sleep, like
/// the Thread machinery); returns the rounded seconds slept.
pub fn kernel_sleep(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0..=1);
    let secs = match args.first() {
        Some(RubyValue::Int(n)) if *n >= 0 => *n as f64,
        Some(RubyValue::Float(f)) if *f >= 0.0 => *f,
        None => {
            panic!("Kernel#sleep without a duration (sleep forever) isn't supported (spike scope)")
        }
        Some(other) => {
            return Err(type_error!(
                "can't convert {} into time interval",
                crate::builtins::class_name_of(other)
            ));
        }
    };
    may::coroutine::sleep(std::time::Duration::from_secs_f64(secs));
    Ok(RubyValue::Int(secs.round() as i64))
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
    std::process::exit(code)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eql_requires_same_class_and_equality() {
        let t = eql_p(&RubyValue::Int(1), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(t, RubyValue::Bool(true)));
        let f = eql_p(&RubyValue::Int(1), &[RubyValue::Float(1.0)], None).unwrap();
        assert!(matches!(f, RubyValue::Bool(false)));
    }

    #[test]
    fn to_s_and_inspect_render_like_puts_and_p() {
        let s = to_s(&RubyValue::Nil, &[], None).unwrap();
        let RubyValue::Str(s) = s else { panic!() };
        assert_eq!(&*s.lock().to_utf8_lossy(), "");
        let i = inspect(&RubyValue::Nil, &[], None).unwrap();
        let RubyValue::Str(i) = i else { panic!() };
        assert_eq!(&*i.lock().to_utf8_lossy(), "nil");
    }

    #[test]
    fn itself_returns_the_receiver_and_freeze_reports_frozen() {
        assert!(matches!(
            itself(&RubyValue::Int(7), &[], None).unwrap(),
            RubyValue::Int(7)
        ));
        assert!(matches!(
            frozen_p(&RubyValue::Int(7), &[], None).unwrap(),
            RubyValue::Bool(true) // immediates are frozen
        ));
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        assert!(matches!(
            frozen_p(&s, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
        freeze(&s, &[], None).unwrap();
        assert!(matches!(
            frozen_p(&s, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }
}
