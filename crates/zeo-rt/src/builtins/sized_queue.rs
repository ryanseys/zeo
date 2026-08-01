//! `SizedQueue < Queue` -- a bounded blocking queue. It owns the bounded
//! constructor `SizedQueue.new(n)` and the bound itself (`max`/`max=`, which
//! an unbounded `Queue` does not answer at all); every other instance row
//! (`push`/`pop`/`close`/...) is inherited from `Queue` through the ancestry
//! walk. Instances are the same `RubyValue::Queue` values, whose runtime
//! payload carries the bound.

use crate::RubyValue;
use crate::builtins::arg_int;
use crate::thread::{queue_max, queue_set_max, sized_queue_new};
use zeo_macros::ruby_class;

ruby_class! {
    SizedQueue = zeo_abi::SIZED_QUEUE_CLASS < zeo_abi::QUEUE_CLASS;

    // `SizedQueue.new(n)` -- the bounded constructor.
    def self."new" cfunc (_recv, arg) {
        Ok(sized_queue_new(arg_int!(arg)))
    }

    // The bound. `Queue` shares the payload field but not these rows, so
    // `Queue.new.max` is the NoMethodError CRuby raises.
    def "max"(recv) {
        Ok(match queue_max(&recv.as_queue_unchecked()) {
            Some(n) => RubyValue::Int(n),
            None => RubyValue::Nil,
        })
    }
    def "max="(recv, arg) {
        queue_set_max(&recv.as_queue_unchecked(), arg_int!(arg));
        Ok((*arg).clone())
    }
    // CRuby defines `clear` and `num_waiting` on BOTH Queue and SizedQueue,
    // so each is listed by its own class; the bodies are Queue's.
    def "clear"(recv) {
        let q = recv.as_queue_unchecked();
        crate::thread::queue_clear(&q);
        Ok(RubyValue::Queue(q))
    }
    def "num_waiting"(recv) {
        Ok(RubyValue::Int(crate::thread::queue_num_waiting(&recv.as_queue_unchecked())))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sized_queue_registers_its_bounded_constructor_and_the_bound() {
        let t = crate::builtins::registered_table(zeo_abi::SIZED_QUEUE_CLASS)
            .expect("SizedQueue is a registered builtin table");
        assert!((t.class.as_ref().unwrap().lookup)("new").is_some());
        // Its instance table holds ONLY the bound; every other row comes from
        // Queue through the ancestry walk (`SizedQueue < Queue`).
        let inst = t.instance.as_ref().expect("SizedQueue owns max/max=");
        assert!((inst.lookup)("max").is_some());
        assert!((inst.lookup)("max=").is_some());
        assert!((inst.lookup)("push").is_none());
    }
}
