//! `Proc` (CRuby proc.c) -- the invocation surface. `rproc` because `proc`
//! is better left unshadowed and the crate already names Proc's payload
//! type `RProc` (`rproc.rs` precedent). `Proc#===` INVOKES the proc (what
//! `case x when ->(v) { ... }` means); leaving it to Kernel's equality
//! default would be silent wrongness.

use crate::RubyValue;
use crate::builtins::arg_error;
use crate::builtins::inherited_row;
use zeo_macros::ruby_class;

fn recv_proc(recv: &RubyValue) -> &crate::RProc {
    match recv {
        RubyValue::Proc(p) => p,
        _ => unreachable!("Proc table row dispatched on a non-Proc receiver"),
    }
}

ruby_class! {
    Proc = zeo_abi::PROC_CLASS < zeo_abi::OBJECT_CLASS;

    // `Proc.new { ... }` / `Proc.new(&b)` -- the block IS the proc, so return
    // it. Without a block, CRuby (3.0+) raises ArgumentError rather than
    // capturing the enclosing method's block.
    // A SUBCLASS receiver (`class P < Proc; end; P.new { }`) re-tags the block
    // as its own class rather than answering a plain Proc -- CRuby allocates
    // through the receiver here too. The value stays a `RubyValue::Proc`, so
    // nothing about calling it changes; see `ProcData::class_id`.
    def self."new" allocs (recv, *_args, &block) {
        match block {
            Some(RubyValue::Proc(p)) => {
                let target = match recv {
                    RubyValue::Class(cid) => *cid,
                    _ => zeo_abi::PROC_CLASS,
                };
                Ok(RubyValue::Proc(p.as_class(target)))
            }
            _ => Err(arg_error!("tried to create Proc object without a block")),
        }
    }

    // `Proc#==`/`#eql?`: same underlying block. `dup`/`clone` share the block,
    // so a copy compares equal (unlike `equal?`, which is allocation identity).
    def "==" arity 1 | "eql?" arity 1 (recv, *args, &_block) {
        let eq = matches!(&args[0], RubyValue::Proc(other) if recv_proc(recv).block_eq(other));
        Ok(RubyValue::Bool(eq))
    }

    def "call" | "[]" | "yield" | "==="(recv, *args, &block) {
        let p = recv_proc(recv);
        // Forward the call-site block to the proc's own `&block` param
        // (`->(&b) { b.call }.call { ... }`); `None` when no block, exactly
        // like a plain `#call`.
        match p.call_with_block(args, block) {
            // A `break` inside a non-lambda proc invoked via `#call` has no
            // iterator to unwind to, so CRuby raises LocalJumpError (`#reason`
            // `:break`). An iterator yielding to a block reaches `RProc::call`
            // directly, NOT this dispatch row, so a legitimate iterator break
            // still propagates as `Signal::Break`. (A lambda folds its own
            // `break` into a normal return and never surfaces one here.)
            Err(crate::Signal::Break(v)) if !p.is_lambda() => {
                Err(crate::dispatch::raise_error_details(
                    "LocalJumpError",
                    "break from proc-closure".to_string(),
                    &[
                        ("reason", RubyValue::Symbol(crate::Symbol::intern("break"))),
                        ("exit_value", v),
                    ],
                ))
            }
            other => other,
        }
    }
    def "to_proc"(recv) {
        Ok(recv.clone())
    }
    // Both read the metadata codegen recorded from the block/lambda's own
    // static `Params` (see `RProc::with_meta`).
    def "arity"(recv) {
        Ok(RubyValue::Int(recv_proc(recv).arity() as i64))
    }
    def "lambda?"(recv) {
        Ok(RubyValue::Bool(recv_proc(recv).is_lambda()))
    }
    // `Proc#binding` -- the scope the block was WRITTEN in: its `self` and
    // its locals, shared by reference (see `ProcData::binding`), never the
    // block's own locals. A fresh Binding object over that same scope on
    // every call, as CRuby's is (`pr.binding.equal?(pr.binding)` is false
    // there, and the two still name one environment). A proc with no
    // captured scope -- a runtime-internal one like `Symbol#to_proc`, or any
    // proc in a program that never mentions `Proc#binding`, which is what
    // lets codegen skip the capture -- is CRuby's C-level proc.
    def "binding"(recv) {
        let Some(b) = recv_proc(recv).binding() else {
            return Err(crate::builtins::arg_error!(
                "Can't create Binding from C level Proc"
            ));
        };
        Ok(crate::builtins::binding::rebind(b))
    }
    // Returns self, and does nothing else -- see `Module#ruby2_keywords`.
    def "ruby2_keywords"(recv) {
        Ok(recv.clone())
    }
    // `source_location` -> `[file, line]` where the block was written, or
    // `nil` for a runtime-internal proc -- CRuby's answer for a C-level Proc,
    // which is what one of ours is.
    def "source_location"(recv) {
        let Some((file, line)) = recv_proc(recv).location() else {
            return Ok(RubyValue::Nil);
        };
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Str(crate::string_new(file.to_string())),
            RubyValue::Int(line as i64),
        ])))
    }
    // `parameters` -- `[[kind, name], ...]` from the static signature codegen
    // recorded. A kind-only entry (anonymous `*`/`**`/`&`) is a one-element
    // array, matching CRuby.
    def "parameters"(recv, arg?) {
        let p = recv_proc(recv);
        // `parameters(lambda:)` forces the reporting view: true reports
        // plain positionals as :req, false as :opt, nil/absent follows the
        // receiver's own lambda-ness. Kinds are stored canonically (lambda
        // style), so a proc-view report demotes every mandatory positional
        // (:req -> :opt); rest/opt/keyword/block kinds never change.
        let lambda_view = match arg {
            Some(RubyValue::Hash(h)) => h
                .lock()
                .values()
                .find_map(|(k, v)| match k {
                    RubyValue::Symbol(s) if s.name() == "lambda" && !v.is_nil() => Some(v.truthy()),
                    _ => None,
                })
                .unwrap_or_else(|| p.is_lambda()),
            _ => p.is_lambda(),
        };
        let rows: Vec<RubyValue> = p
            .parameters()
            .iter()
            .map(|pm| {
                let kind = if pm.kind == "req" && !lambda_view { "opt" } else { pm.kind };
                let mut entry = vec![RubyValue::Symbol(crate::Symbol::intern(kind))];
                if let Some(name) = pm.name {
                    entry.push(RubyValue::Symbol(crate::Symbol::intern(name)));
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
    def "curry"(recv, arg?) {
        let p = recv_proc(recv).clone();
        let n = match arg {
            // An explicit nil arity is accepted as absent (CRuby's
            // proc_curry); anything else through the `to_int` protocol.
            // A negative (optional/rest) arity has no fixed slot count to
            // curry toward -- CRuby uses `-arity - 1`, the required count.
            None | Some(RubyValue::Nil) => {
                let a = p.arity();
                if a < 0 { (-a - 1) as i64 } else { a as i64 }
            }
            Some(v) => {
                let n = crate::builtins::convert::to_index(v)?;
                // Only a LAMBDA is checked, and only against slots it really
                // has: `proc_curry` rejects an arity outside `min..max`, but a
                // plain proc is elastic and takes any. A negative `arity` means
                // a splat, so the maximum is unbounded and only the minimum
                // (`-arity - 1`) can be violated.
                if p.is_lambda() {
                    let a = p.arity();
                    let (min, max) = match a < 0 {
                        true => ((-a - 1) as i64, i64::MAX),
                        false => (a as i64, a as i64),
                    };
                    if n < min || n > max {
                        return Err(arg_error!(
                            "wrong number of arguments (given {n}, expected {min})"
                        ));
                    }
                }
                n
            }
        };
        Ok(curried(p, Vec::new(), n as usize))
    }
    // Function composition. `(f >> g).call(x)` is `g.call(f.call(x))`;
    // `(f << g).call(x)` is `f.call(g.call(x))`. The other operand is any
    // callable (Proc, Method, ...), invoked through its own `call`; the
    // result is a var-args lambda.
    def ">>"(recv, other) {
        let f = recv_proc(recv).clone();
        // The composed proc's lambda-ness follows the FIRST function to run:
        // for `f >> g` that is the receiver `f` (CRuby's proc_compose).
        let is_lambda = f.is_lambda();
        let g = (*other).clone();
        Ok(RubyValue::Proc(crate::RProc::with_meta(
            move |a: &[RubyValue]| {
                let mid = f.call(a)?;
                crate::dispatch::send_value(&g, crate::Symbol::intern("call"), &[mid], None)
            },
            -1,
            is_lambda,
        )))
    }
    def "<<"(recv, other) {
        let f = recv_proc(recv).clone();
        let g = (*other).clone();
        // For `f << g`, `g` runs first, so the composition follows the
        // ARGUMENT's lambda-ness (a non-Proc callable, e.g. a Method, is
        // lambda-like -> true).
        let is_lambda = match &g {
            RubyValue::Proc(p) => p.is_lambda(),
            _ => true,
        };
        Ok(RubyValue::Proc(crate::RProc::with_meta(
            move |a: &[RubyValue]| {
                let mid = crate::dispatch::send_value(&g, crate::Symbol::intern("call"), a, None)?;
                f.call(&[mid])
            },
            -1,
            is_lambda,
        )))
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "clone"(recv) { inherited_row!(kernel, "clone", recv, __args, None) }
    def "dup"(recv) { inherited_row!(kernel, "dup", recv, __args, None) }
    def "hash"(recv) { inherited_row!(kernel, "hash", recv, __args, None) }
    def "inspect"(recv) { Ok(RubyValue::Str(crate::string_new(proc_inspect(recv_proc(recv))))) }
    // Genuinely the same string here -- oracle-verified -- unlike `Complex`,
    // `Rational` and `Regexp`, which spell the two differently.
    def "to_s"(recv) { Ok(RubyValue::Str(crate::string_new(proc_inspect(recv_proc(recv))))) }
}

/// `Proc#inspect`/`#to_s` -- what tells two procs apart: the identity, where
/// it was written, and whether it is a lambda.
///
///     #<Proc:0x00000001234 file.rb:3>
///     #<Proc:0x00000001234 file.rb:4 (lambda)>
///     #<Proc:0x00000001234 (lambda)>        -- a runtime-internal proc
///
/// A proc with no recorded location is CRuby's C-level Proc, which prints
/// the identity alone. A `Symbol#to_proc` carries its symbol and renders as
/// `#<Proc:0x...(&:upcase) (lambda)>` -- no space before the `(&:`.
fn proc_inspect(p: &crate::RProc) -> String {
    let lambda = if p.is_lambda() { " (lambda)" } else { "" };
    let addr = p.identity();
    if let Some(sym) = p.symbol_origin() {
        return format!("#<Proc:0x{addr:016x}(&:{}){lambda}>", sym.name());
    }
    match p.location() {
        Some((file, line)) => format!("#<Proc:0x{addr:016x} {file}:{line}{lambda}>"),
        None => format!("#<Proc:0x{addr:016x}{lambda}>"),
    }
}

/// One step of `Proc#curry`: a proc that either invokes the target (enough
/// arguments collected) or answers the next curried step.
fn curried(target: crate::RProc, collected: Vec<RubyValue>, want: usize) -> RubyValue {
    // A curried proc keeps the target's lambda-ness (`proc{}.curry.lambda?` is
    // false, `lambda{}.curry.lambda?` is true -- oracle-verified).
    let is_lambda = target.is_lambda();
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
        // and `.curry[1].arity` are both -1 -- oracle-verified.
        -1,
        is_lambda,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::PROC_CLASS)
            .unwrap()
            .instance
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }

    #[test]
    fn case_eq_invokes_the_proc() {
        let doubler: crate::RProc = crate::RProc::new(|args: &[RubyValue]| {
            let RubyValue::Int(i) = &args[0] else {
                panic!()
            };
            Ok(RubyValue::Int(i * 2))
        });
        let p = RubyValue::Proc(doubler);
        let r = imethod("call")(&p, &[RubyValue::Int(21)], None).unwrap();
        assert!(matches!(r, RubyValue::Int(42)));
        assert!(lookup("===").is_some());
    }

    fn adder3() -> RubyValue {
        RubyValue::Proc(crate::RProc::with_meta(
            |args: &[RubyValue]| {
                let sum = args
                    .iter()
                    .map(|a| match a {
                        RubyValue::Int(i) => *i,
                        _ => panic!("ints only"),
                    })
                    .sum();
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
        assert!(lookup("yield").is_some());
        assert!(lookup("yield").is_some());
        let r = imethod("call")(&adder3(), &[RubyValue::Int(1), RubyValue::Int(2)], None).unwrap();
        assert!(matches!(r, RubyValue::Int(3)));
    }

    /// `arity`/`lambda?` read the metadata codegen recorded from the
    /// block's own static parameters -- see `zeo`'s `proc_arity`, whose
    /// own tests cover the 26 signature shapes against the ruby oracle.
    #[test]
    fn arity_and_lambda_p_read_the_recorded_metadata() {
        let p = adder3();
        assert_eq!(
            imethod("arity")(&p, &[], None).unwrap().inspect_string(),
            "3"
        );
        assert_eq!(
            imethod("lambda?")(&p, &[], None).unwrap().inspect_string(),
            "true"
        );

        let plain = RubyValue::Proc(crate::RProc::new(|_| Ok(RubyValue::Nil)));
        assert_eq!(
            imethod("arity")(&plain, &[], None)
                .unwrap()
                .inspect_string(),
            "-1"
        );
        assert_eq!(
            imethod("lambda?")(&plain, &[], None)
                .unwrap()
                .inspect_string(),
            "false"
        );
    }

    /// `curry` collects arguments until the arity is satisfied, then calls.
    #[test]
    fn curry_collects_arguments_across_calls() {
        let curried = imethod("curry")(&adder3(), &[], None).unwrap();
        let step1 = imethod("call")(&curried, &[RubyValue::Int(1)], None).unwrap();
        let step2 = imethod("call")(&step1, &[RubyValue::Int(2)], None).unwrap();
        let out = imethod("call")(&step2, &[RubyValue::Int(3)], None).unwrap();
        assert!(matches!(out, RubyValue::Int(6)));
    }

    /// Several arguments at once are fine, and a full application invokes
    /// immediately.
    #[test]
    fn curry_accepts_grouped_arguments() {
        let curried = imethod("curry")(&adder3(), &[], None).unwrap();
        let step =
            imethod("call")(&curried, &[RubyValue::Int(1), RubyValue::Int(2)], None).unwrap();
        let out = imethod("call")(&step, &[RubyValue::Int(3)], None).unwrap();
        assert!(matches!(out, RubyValue::Int(6)));

        let at_once = imethod("call")(
            &imethod("curry")(&adder3(), &[], None).unwrap(),
            &[RubyValue::Int(1), RubyValue::Int(2), RubyValue::Int(3)],
            None,
        )
        .unwrap();
        assert!(matches!(at_once, RubyValue::Int(6)));
    }

    /// Each partial application is a FRESH proc -- reusing one must not
    /// accumulate arguments from a previous chain.
    #[test]
    fn each_curry_step_is_independent() {
        let step = imethod("call")(
            &imethod("curry")(&adder3(), &[], None).unwrap(),
            &[RubyValue::Int(10)],
            None,
        )
        .unwrap();
        let a = imethod("call")(
            &imethod("call")(&step, &[RubyValue::Int(1)], None).unwrap(),
            &[RubyValue::Int(2)],
            None,
        )
        .unwrap();
        let b = imethod("call")(
            &imethod("call")(&step, &[RubyValue::Int(3)], None).unwrap(),
            &[RubyValue::Int(4)],
            None,
        )
        .unwrap();
        assert!(matches!(a, RubyValue::Int(13)));
        assert!(matches!(b, RubyValue::Int(17)));
    }

    /// Every curry STEP reports var-args arity and is a lambda (CRuby builds
    /// each from a C function taking `*args`) -- oracle-verified.
    #[test]
    fn a_curried_proc_reports_var_args_arity_and_is_a_lambda() {
        let curried = imethod("curry")(&adder3(), &[], None).unwrap();
        assert_eq!(
            imethod("arity")(&curried, &[], None)
                .unwrap()
                .inspect_string(),
            "-1"
        );
        assert_eq!(
            imethod("lambda?")(&curried, &[], None)
                .unwrap()
                .inspect_string(),
            "true"
        );
    }

    /// An explicit count curries a proc whose own arity is unbounded.
    #[test]
    fn curry_takes_an_explicit_count() {
        let var_args = RubyValue::Proc(crate::RProc::new(|args: &[RubyValue]| {
            Ok(RubyValue::Int(args.len() as i64))
        }));
        let curried = imethod("curry")(&var_args, &[RubyValue::Int(2)], None).unwrap();
        let step = imethod("call")(&curried, &[RubyValue::Int(1)], None).unwrap();
        let out = imethod("call")(&step, &[RubyValue::Int(2)], None).unwrap();
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
        let out = imethod("call")(
            &imethod("curry")(&p, &[], None).unwrap(),
            &[RubyValue::Int(9)],
            None,
        )
        .unwrap();
        assert!(matches!(out, RubyValue::Int(1)));
    }
}
