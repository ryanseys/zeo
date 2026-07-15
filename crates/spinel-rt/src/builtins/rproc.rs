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
        recv_proc(recv)(args)
    }
    "to_proc" => fn to_proc(recv, args, _block) {
        crate::builtins::arity!(args, 0);
        Ok(recv.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn case_eq_invokes_the_proc() {
        let doubler: crate::RProc = Arc::new(|args: &[RubyValue]| {
            let RubyValue::Int(i) = &args[0] else { panic!() };
            Ok(RubyValue::Int(i * 2))
        });
        let p = RubyValue::Proc(doubler);
        let r = call(&p, &[RubyValue::Int(21)], None).unwrap();
        assert!(matches!(r, RubyValue::Int(42)));
        assert!(lookup("===").is_some());
    }
}
