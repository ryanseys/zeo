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

use crate::builtins::{arity, block_or_enum, builtin_methods, need_block};
use crate::{RubyValue, Signal, Symbol};

/// A `Vec<Symbol>` as a Ruby Array of Symbols -- reflection's return shape.
fn syms_to_array(names: Vec<Symbol>) -> RubyValue {
    RubyValue::Array(crate::array_new(names.into_iter().map(RubyValue::Symbol).collect()))
}

/// Order-preserving dedup for a combined symbol list (each of the two source
/// lists is already internally deduped; this merges them).
fn dedup_syms(names: Vec<Symbol>) -> Vec<Symbol> {
    let mut seen = std::collections::HashSet::new();
    names.into_iter().filter(|s| seen.insert(*s)).collect()
}

builtin_methods! {
    pub(crate) fn lookup;

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
    "pp" => fn pp(_recv, args, _block) {
        kernel_pp(args)
    }
    "warn" => fn warn(_recv, args, _block) {
        kernel_warn(args)
    }
    // `Kernel#method(:name)` -- a bound Method object (see
    // `builtins::method_obj`). Reaches every receiver via the MRO walk's
    // Kernel row, including the top-level `main` object.
    "method" => fn method(recv, args, _block) {
        arity!(args, 1);
        crate::builtins::method_obj::method_new(recv, &args[0])
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
                return Err(crate::dispatch::raise_error(
                    "NotImplementedError",
                    "eval with an explicit binding is not supported yet".to_string(),
                ));
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
            crate::dispatch::raise_error("LocalJumpError", "no block given (yield)".to_string())
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
            RubyValue::Nil => 8,
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
    "nil?" => fn nil_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv.is_nil()))
    }
    "itself" => fn itself(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
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
    "to_s" => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(recv.to_display_string())))
    }
    "inspect" => fn inspect(recv, args, _block) {
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
            return Err(crate::dispatch::raise_error(
                "TypeError",
                "class or module required".to_string(),
            ));
        };
        Ok(RubyValue::Bool(crate::dispatch::is_a(recv.class_id(), *target)))
    }
    "instance_of?" => fn instance_of_p(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Class(target) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                "class or module required".to_string(),
            ));
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
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("{} is not a symbol nor a string", other.inspect_string()),
                ))
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
        names.extend(crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::MethodVisibility::Public,
            inherit,
        ));
        Ok(syms_to_array(dedup_syms(names)))
    }
    "private_methods" => fn private_methods_m(recv, args, _block) {
        arity!(args, 0..=1);
        let inherit = !matches!(args.first(), Some(RubyValue::Bool(false)) | Some(RubyValue::Nil));
        let names = crate::dispatch::instance_method_names(
            recv.class_id(),
            crate::dispatch::MethodVisibility::Private,
            inherit,
        );
        Ok(syms_to_array(names))
    }
    // No separate protected tracking in this runtime (documented) -- empty.
    "protected_methods" => fn protected_methods_m(_recv, args, _block) {
        arity!(args, 0..=1);
        Ok(syms_to_array(Vec::new()))
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
    // args), CRuby's obj_to_enum (Phase 17.2). The block-as-size-proc
    // form is Tier B (rare; the stored-size Enumerator.new form covers
    // the practical cases).
    "to_enum" | "enum_for" => fn to_enum(recv, args, _block) {
        let meth = match args.first() {
            None => "each".to_string(),
            Some(RubyValue::Symbol(s)) => s.name().as_str().to_string(),
            Some(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
            Some(other) => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("{} is not a symbol nor a string", other.inspect_string()),
                ))
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
        Some(RubyValue::Int(b)) => Some(*b as u32),
        Some(other) => {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(other)
                ),
            ))
        }
        None => None,
    };
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
            parse_integer_strict(&text, base).ok_or_else(|| {
                crate::dispatch::raise_error(
                    "ArgumentError",
                    format!("invalid value for Integer(): {:?}", text),
                )
            })
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Integer",
                if matches!(other, RubyValue::Nil) {
                    "nil".to_string()
                } else {
                    crate::builtins::class_name_of(other)
                }
            ),
        )),
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
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__")
    {
        return None;
    }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
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
            Ok(RubyValue::Float(crate::builtins::numeric::num_to_f64_unchecked(&args[0])))
        }
        RubyValue::Str(s) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            let clean: String = text.trim().chars().filter(|c| *c != '_').collect();
            clean
                .parse::<f64>()
                .ok()
                .filter(|f| f.is_finite() || clean.to_ascii_lowercase().contains("inf"))
                .map(RubyValue::Float)
                .ok_or_else(|| {
                    crate::dispatch::raise_error(
                        "ArgumentError",
                        format!("invalid value for Float(): {:?}", text),
                    )
                })
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Float",
                if matches!(other, RubyValue::Nil) {
                    "nil".to_string()
                } else {
                    crate::builtins::class_name_of(other)
                }
            ),
        )),
    }
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
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "can't convert {} into Rational",
                    crate::builtins::class_name_of(other)
                ),
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

/// `Kernel#Complex(real, imag = 0)`.
pub fn kernel_complex(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    let imag = args.get(1).cloned().unwrap_or(RubyValue::Int(0));
    crate::builtins::complex::complex_new(args[0].clone(), imag)
}

/// `Kernel#String(arg)` -- `to_s` (the `to_str`-first nuance is invisible
/// for builtin receivers).
pub fn kernel_string(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    Ok(RubyValue::Str(crate::string_new(args[0].to_display_string())))
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
                .map(|(k, v)| {
                    RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()]))
                })
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
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Hash",
                crate::builtins::class_name_of(other)
            ),
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
    let mut buf = String::new();
    for a in args {
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
        return Err(crate::dispatch::raise_error(
            "TypeError",
            "no format string given".to_string(),
        ));
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
        Some(RubyValue::Int(n)) => RubyValue::Int(-((r % (n.unsigned_abs())) as i64)),
        Some(RubyValue::Float(x)) => {
            RubyValue::Float((r >> 11) as f64 / (1u64 << 53) as f64 * x)
        }
        Some(other) => {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("invalid argument - {}", other.to_display_string()),
            ))
        }
    })
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
    Ok(crate::builtins::integer::int_value(num_bigint::BigInt::from(previous)))
}

/// `Kernel#catch(tag) { ... }` / `Kernel#throw(tag[, value])`.
pub fn kernel_catch(tag: RubyValue, block: RubyValue) -> Result<RubyValue, Signal> {
    let RubyValue::Proc(p) = &block else {
        panic!("Kernel#catch requires a block");
    };
    match p.call(std::slice::from_ref(&tag)) {
        Err(Signal::Throw(t, v)) if t.rb_eq(&tag) => Ok(v),
        other => other,
    }
}

pub fn kernel_throw(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    Err(Signal::Throw(
        args[0].clone(),
        args.get(1).cloned().unwrap_or(RubyValue::Nil),
    ))
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
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "can't convert {} into time interval",
                    crate::builtins::class_name_of(other)
                ),
            ))
        }
    };
    may::coroutine::sleep(std::time::Duration::from_secs_f64(secs));
    Ok(RubyValue::Int(secs.round() as i64))
}

/// `Kernel#exit` / `Kernel#abort` -- direct process exit (CRuby raises
/// SystemExit through at_exit handlers; both are documented scope-cuts).
pub fn kernel_exit(args: &[RubyValue]) -> ! {
    let code = match args.first() {
        None | Some(RubyValue::Bool(true)) => 0,
        Some(RubyValue::Bool(false)) => 1,
        Some(RubyValue::Int(n)) => *n as i32,
        Some(_) => 0,
    };
    // `at_exit` handlers run on an explicit `exit` too (CRuby's rule;
    // `exit!` would skip them, but that maps to `abort`-family here).
    crate::exec::run_at_exit();
    std::process::exit(code)
}

pub fn kernel_abort(args: &[RubyValue]) -> ! {
    if let Some(msg) = args.first() {
        eprintln!("{}", msg.to_display_string());
    }
    std::process::exit(1)
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
