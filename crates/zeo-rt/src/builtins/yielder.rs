//! `Enumerator::Yielder` -- the object handed to an `Enumerator.new { |y| ... }`
//! generator block. It wraps the consumer's `RProc` (a `RubyValue::Yielder`
//! variant): `y << v` and `y.yield(*vs)` forward to it, `y.to_proc` unwraps it.
//! The Enumerator machinery that creates and drives yielders lives in
//! `enumerator.rs`.

use crate::RProc;
use crate::RubyValue;
use crate::builtins::arity;
use zeo_macros::ruby_class;

fn recv_yielder(recv: &RubyValue) -> &RProc {
    match recv {
        RubyValue::Yielder(p) => p,
        _ => unreachable!("Yielder table row dispatched on a non-Yielder receiver"),
    }
}

ruby_class! {
    Yielder = zeo_abi::YIELDER_CLASS < zeo_abi::OBJECT_CLASS;

    // `y << v` forwards to the consumer's block and returns the yielder
    // (chainable: `y << 1 << 2`).
    def "<<"(recv, args, _block) {
        recv_yielder(recv).call(args)?;
        Ok(recv.clone())
    }
    // `y.yield(*vs)` forwards and returns the block's own return value.
    def "yield"(recv, args, _block) {
        recv_yielder(recv).call(args)
    }
    def "to_proc"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Proc(recv_yielder(recv).clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::YIELDER_CLASS)
            .expect("Yielder is a registered builtin table")
            .instance
            .as_ref()
            .expect("Yielder has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Yielder#{name} is defined"))
    }

    #[test]
    fn push_chains_and_yield_returns_the_block_value() {
        let blk: RProc = RProc::new(|_raw| Ok(RubyValue::Int(42)));
        let y = RubyValue::Yielder(blk);
        let back = imethod("<<")(&y, &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(back, RubyValue::Yielder(_)));
        assert!(matches!(
            imethod("yield")(&y, &[RubyValue::Int(1)], None).unwrap(),
            RubyValue::Int(42)
        ));
    }
}
