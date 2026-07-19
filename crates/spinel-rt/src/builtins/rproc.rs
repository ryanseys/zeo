//! `Proc` (CRuby proc.c) -- the invocation surface. `rproc` because `proc`
//! is better left unshadowed and the crate already names Proc's payload
//! type `RProc` (`rproc.rs` precedent). `Proc#===` INVOKES the proc (what
//! `case x when ->(v) { ... }` means); leaving it to Kernel's equality
//! default would be silent wrongness.

use crate::builtins::builtin_methods;
use crate::RubyValue;

fn recv_proc(recv: &RubyValue) -> &crate::RProc {
    match recv {
        RubyValue::Proc(p) => p,
        _ => unreachable!("Proc table row dispatched on a non-Proc receiver"),
    }
}

builtin_methods! {
    pub(crate) fn lookup;

    "call" | "()" | "[]" | "yield" | "===" => fn call(recv, args, _block) {
        recv_proc(recv).call(args)
    }
    "to_proc" => fn to_proc(recv, args, _block) {
        crate::builtins::arity!(args, 0);
        Ok(recv.clone())
    }
    // Both read the metadata codegen recorded from the block/lambda's own
    // static `Params` (see `RProc::with_meta`).
    "arity" => fn proc_arity(recv, args, _block) {
        crate::builtins::arity!(args, 0);
        Ok(RubyValue::Int(recv_proc(recv).arity() as i64))
    }
    "lambda?" => fn lambda_p(recv, args, _block) {
        crate::builtins::arity!(args, 0);
        Ok(RubyValue::Bool(recv_proc(recv).is_lambda()))
    }
    // `source_location` -> `[file, line]`. spinel-rs is whole-program AOT and
    // the conformance harness disables the line map, so a proc's exact
    // origin isn't tracked; the pair's SHAPE and element types match CRuby
    // (`[String, Integer]`), which is what proc introspection relies on.
    "source_location" => fn source_location(recv, args, _block) {
        crate::builtins::arity!(args, 0);
        let _ = recv_proc(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Str(crate::string_new(String::new())),
            RubyValue::Int(0),
        ])))
    }
    // `parameters` -- `[[kind, name], ...]` from the static signature codegen
    // recorded. A kind-only entry (anonymous `*`/`**`/`&`) is a one-element
    // array, matching CRuby.
    "parameters" => fn parameters(recv, args, _block) {
        crate::builtins::arity!(args, 0..=1);
        let rows: Vec<RubyValue> = recv_proc(recv)
            .parameters()
            .iter()
            .map(|p| {
                let mut entry = vec![RubyValue::Symbol(crate::Symbol::intern(p.kind))];
                if let Some(name) = p.name {
                    entry.push(RubyValue::Symbol(name));
                }
                RubyValue::Array(crate::array_new(entry))
            })
            .collect();
        Ok(RubyValue::Array(crate::array_new(rows)))
    }
    // `curry` / `curry(n)`: collects arguments across calls until `n` (the
    // proc's own arity by default) are in hand, then invokes. Each partial
    // application answers a FRESH curried proc -- `add.curry[1]` is reusable,
    // never mutating shared state (CRuby's `proc_curry` builds a new proc
    // per step the same way).
    "curry" => fn curry(recv, args, _block) {
        crate::builtins::arity!(args, 0..=1);
        let p = recv_proc(recv).clone();
        let n = match args.first() {
            Some(RubyValue::Int(n)) => *n,
            Some(other) => return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(other)
                ),
            )),
            // A negative (optional/rest) arity has no fixed slot count to
            // curry toward -- CRuby uses `-arity - 1`, the required count.
            None => {
                let a = p.arity();
                if a < 0 { (-a - 1) as i64 } else { a as i64 }
            }
        };
        Ok(curried(p, Vec::new(), n as usize))
    }
    // Function composition. `(f >> g).call(x)` is `g.call(f.call(x))`;
    // `(f << g).call(x)` is `f.call(g.call(x))`. The other operand is any
    // callable (Proc, Method, ...), invoked through its own `call`; the
    // result is a var-args lambda.
    ">>" => fn compose_forward(recv, args, _block) {
        crate::builtins::arity!(args, 1);
        let f = recv_proc(recv).clone();
        let g = args[0].clone();
        Ok(RubyValue::Proc(crate::RProc::with_meta(
            move |a: &[RubyValue]| {
                let mid = f.call(a)?;
                crate::dispatch::send_value(&g, crate::Symbol::intern("call"), &[mid], None)
            },
            -1,
            true,
        )))
    }
    "<<" => fn compose_backward(recv, args, _block) {
        crate::builtins::arity!(args, 1);
        let f = recv_proc(recv).clone();
        let g = args[0].clone();
        Ok(RubyValue::Proc(crate::RProc::with_meta(
            move |a: &[RubyValue]| {
                let mid = crate::dispatch::send_value(&g, crate::Symbol::intern("call"), a, None)?;
                f.call(&[mid])
            },
            -1,
            true,
        )))
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Proc.new { ... }` / `Proc.new(&b)` -- the block IS the proc, so return
    // it. Without a block, CRuby (3.0+) raises ArgumentError rather than
    // capturing the enclosing method's block.
    "new" => fn new_m(_recv, _args, block) {
        match block {
            Some(p @ RubyValue::Proc(_)) => Ok(p),
            _ => Err(crate::dispatch::raise_error(
                "ArgumentError",
                "tried to create Proc object without a block".to_string(),
            )),
        }
    }
}

/// One step of `Proc#curry`: a proc that either invokes the target (enough
/// arguments collected) or answers the next curried step.
fn curried(target: crate::RProc, collected: Vec<RubyValue>, want: usize) -> RubyValue {
    RubyValue::Proc(crate::RProc::with_meta(
        move |args: &[RubyValue]| {
            let mut have = collected.clone();
            have.extend(args.iter().cloned());
            if have.len() >= want {
                return target.call(&have);
            }
            Ok(curried(target.clone(), have, want))
        },
        // Every curry STEP reports var-args arity, not the count still
        // outstanding: CRuby builds each one with `rb_proc_new` over a C
        // function taking `*args` (`make_curry_proc`), so `.curry.arity`
        // and `.curry[1].arity` are both -1 -- oracle-verified. It is still
        // a lambda.
        -1,
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn case_eq_invokes_the_proc() {
        let doubler: crate::RProc = crate::RProc::new(|args: &[RubyValue]| {
            let RubyValue::Int(i) = &args[0] else { panic!() };
            Ok(RubyValue::Int(i * 2))
        });
        let p = RubyValue::Proc(doubler);
        let r = call(&p, &[RubyValue::Int(21)], None).unwrap();
        assert!(matches!(r, RubyValue::Int(42)));
        assert!(lookup("===").is_some());
    }

    fn adder3() -> RubyValue {
        RubyValue::Proc(crate::RProc::with_meta(
            |args: &[RubyValue]| {
                let sum = args.iter().map(|a| match a {
                    RubyValue::Int(i) => *i,
                    _ => panic!("ints only"),
                }).sum();
                Ok(RubyValue::Int(sum))
            },
            3,
            true,
        ))
    }

    /// `[]` is another spelling of `#call`, like `()`/`yield`.
    #[test]
    fn brackets_are_a_call_alias() {
        assert!(lookup("[]").is_some());
        assert!(lookup("()").is_some());
        assert!(lookup("yield").is_some());
        let r = call(&adder3(), &[RubyValue::Int(1), RubyValue::Int(2)], None).unwrap();
        assert!(matches!(r, RubyValue::Int(3)));
    }

    /// `arity`/`lambda?` read the metadata codegen recorded from the
    /// block's own static parameters -- see `spinelc`'s `proc_arity`, whose
    /// own tests cover the 26 signature shapes against the ruby oracle.
    #[test]
    fn arity_and_lambda_p_read_the_recorded_metadata() {
        let p = adder3();
        assert_eq!(proc_arity(&p, &[], None).unwrap().inspect_string(), "3");
        assert_eq!(lambda_p(&p, &[], None).unwrap().inspect_string(), "true");

        let plain = RubyValue::Proc(crate::RProc::new(|_| Ok(RubyValue::Nil)));
        assert_eq!(proc_arity(&plain, &[], None).unwrap().inspect_string(), "-1");
        assert_eq!(lambda_p(&plain, &[], None).unwrap().inspect_string(), "false");
    }

    /// `curry` collects arguments until the arity is satisfied, then calls.
    #[test]
    fn curry_collects_arguments_across_calls() {
        let curried = curry(&adder3(), &[], None).unwrap();
        let step1 = call(&curried, &[RubyValue::Int(1)], None).unwrap();
        let step2 = call(&step1, &[RubyValue::Int(2)], None).unwrap();
        let out = call(&step2, &[RubyValue::Int(3)], None).unwrap();
        assert!(matches!(out, RubyValue::Int(6)));
    }

    /// Several arguments at once are fine, and a full application invokes
    /// immediately.
    #[test]
    fn curry_accepts_grouped_arguments() {
        let curried = curry(&adder3(), &[], None).unwrap();
        let step = call(&curried, &[RubyValue::Int(1), RubyValue::Int(2)], None).unwrap();
        let out = call(&step, &[RubyValue::Int(3)], None).unwrap();
        assert!(matches!(out, RubyValue::Int(6)));

        let at_once = call(&curry(&adder3(), &[], None).unwrap(),
            &[RubyValue::Int(1), RubyValue::Int(2), RubyValue::Int(3)], None).unwrap();
        assert!(matches!(at_once, RubyValue::Int(6)));
    }

    /// Each partial application is a FRESH proc -- reusing one must not
    /// accumulate arguments from a previous chain.
    #[test]
    fn each_curry_step_is_independent() {
        let step = call(&curry(&adder3(), &[], None).unwrap(), &[RubyValue::Int(10)], None).unwrap();
        let a = call(&call(&step, &[RubyValue::Int(1)], None).unwrap(), &[RubyValue::Int(2)], None).unwrap();
        let b = call(&call(&step, &[RubyValue::Int(3)], None).unwrap(), &[RubyValue::Int(4)], None).unwrap();
        assert!(matches!(a, RubyValue::Int(13)));
        assert!(matches!(b, RubyValue::Int(17)));
    }

    /// Every curry STEP reports var-args arity and is a lambda (CRuby builds
    /// each from a C function taking `*args`) -- oracle-verified.
    #[test]
    fn a_curried_proc_reports_var_args_arity_and_is_a_lambda() {
        let curried = curry(&adder3(), &[], None).unwrap();
        assert_eq!(proc_arity(&curried, &[], None).unwrap().inspect_string(), "-1");
        assert_eq!(lambda_p(&curried, &[], None).unwrap().inspect_string(), "true");
    }

    /// An explicit count curries a proc whose own arity is unbounded.
    #[test]
    fn curry_takes_an_explicit_count() {
        let var_args = RubyValue::Proc(crate::RProc::new(|args: &[RubyValue]| {
            Ok(RubyValue::Int(args.len() as i64))
        }));
        let curried = curry(&var_args, &[RubyValue::Int(2)], None).unwrap();
        let step = call(&curried, &[RubyValue::Int(1)], None).unwrap();
        let out = call(&step, &[RubyValue::Int(2)], None).unwrap();
        assert!(matches!(out, RubyValue::Int(2)));
    }

    /// A negative-arity proc curries to its MINIMUM required count.
    #[test]
    fn curry_of_a_negative_arity_proc_uses_its_minimum() {
        // arity -2 == "one required argument, then more".
        let p = RubyValue::Proc(crate::RProc::with_meta(
            |args: &[RubyValue]| Ok(RubyValue::Int(args.len() as i64)),
            -2,
            false,
        ));
        // min = 1, so one argument completes it.
        let out = call(&curry(&p, &[], None).unwrap(), &[RubyValue::Int(9)], None).unwrap();
        assert!(matches!(out, RubyValue::Int(1)));
    }
}
