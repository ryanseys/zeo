//! `Queue`'s Path-2 (runtime `send`) rows -- for a dynamically-typed Queue
//! receiver (a queue held in an Array/Hash/ivar, or a `Poly` `send` target).
//! The static Path-1 codegen arm (`codegen::call`) emits `queue_push`/
//! `queue_pop`/... directly and never reaches here; these rows mirror it
//! exactly -- `push` to a closed queue is a `ClosedQueueError`, `pop` blocks
//! until an element arrives, a closed empty queue pops `nil` -- so both paths
//! agree. `Queue` has a dedicated `RubyValue::Queue` variant (like
//! Thread/Fiber), unwrapped with `as_queue_unchecked`. `SizedQueue` (a
//! sibling file) inherits every instance row here through the ancestry walk,
//! since `SizedQueue < Queue`.

use crate::RubyValue;
use crate::builtins::{arg_int, arity};
use crate::dispatch::raise_error;
use crate::thread::{
    queue_close, queue_closed, queue_len, queue_max, queue_new, queue_pop, queue_push,
    queue_set_max,
};
use zeo_macros::ruby_class;

ruby_class! {
    Queue = zeo_abi::QUEUE_CLASS < zeo_abi::OBJECT_CLASS;

    def self."new"(_recv, args, _block) {
        arity!(args, 0);
        Ok(queue_new())
    }

    // `push`/`<<`/`enq` enqueue one value and return self; pushing to a
    // closed queue is a `ClosedQueueError`.
    def "push" | "<<" | "enq" (recv, args, _block) {
        arity!(args, 1);
        let q = recv.as_queue_unchecked();
        match queue_push(&q, args[0].clone()) {
            Ok(()) => Ok(RubyValue::Queue(q)),
            Err(_) => Err(raise_error("ClosedQueueError", "queue closed".to_string())),
        }
    }
    // `pop`/`shift`/`deq` -- block (yielding) while empty and open; a closed
    // empty queue pops `nil`.
    def "pop" | "shift" | "deq" (recv, args, _block) {
        arity!(args, 0);
        queue_pop(&recv.as_queue_unchecked())
    }
    def "close"(recv, args, _block) {
        arity!(args, 0);
        let q = recv.as_queue_unchecked();
        queue_close(&q);
        Ok(RubyValue::Queue(q))
    }
    def "closed?"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(queue_closed(&recv.as_queue_unchecked())))
    }
    def "length" | "size" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(queue_len(&recv.as_queue_unchecked())))
    }
    def "empty?"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(queue_len(&recv.as_queue_unchecked()) == 0))
    }
    // `SizedQueue#max`/`max=` -- the bound. `max` on an unbounded `Queue`
    // answers `nil` (a documented divergence: CRuby has no `Queue#max`).
    def "max"(recv, args, _block) {
        arity!(args, 0);
        Ok(match queue_max(&recv.as_queue_unchecked()) {
            Some(n) => RubyValue::Int(n),
            None => RubyValue::Nil,
        })
    }
    def "max="(recv, args, _block) {
        arity!(args, 1);
        queue_set_max(&recv.as_queue_unchecked(), arg_int!(args, 0));
        Ok(args[0].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::queue_new;

    /// The `ruby_class!`-generated methods are reachable only through the
    /// dispatch tables (their Rust fn names are mangled), so the tests call
    /// them the way real dispatch does -- through Queue's registered lookups.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::QUEUE_CLASS)
            .expect("Queue is a registered builtin table")
            .instance
            .as_ref()
            .expect("Queue has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Queue#{name} is defined"))
    }

    #[test]
    fn the_table_covers_the_dynamic_queue_surface() {
        let t = crate::builtins::registered_table(zeo_abi::QUEUE_CLASS).unwrap();
        let inst = t.instance.as_ref().unwrap();
        assert!((inst.lookup)("push").is_some());
        assert!((inst.lookup)("<<").is_some());
        assert!((inst.lookup)("pop").is_some());
        assert!((inst.lookup)("nope").is_none());
        assert!((t.class.as_ref().unwrap().lookup)("new").is_some());
    }

    #[test]
    fn push_len_pop_empty_round_trip() {
        let q = queue_new();
        assert_eq!(
            imethod("empty?")(&q, &[], None).unwrap().inspect_string(),
            "true"
        );
        imethod("push")(&q, &[RubyValue::Int(1)], None).unwrap();
        imethod("push")(&q, &[RubyValue::Int(2)], None).unwrap();
        assert_eq!(
            imethod("length")(&q, &[], None).unwrap().inspect_string(),
            "2"
        );
        assert_eq!(imethod("pop")(&q, &[], None).unwrap().inspect_string(), "1");
        assert_eq!(imethod("pop")(&q, &[], None).unwrap().inspect_string(), "2");
        assert_eq!(
            imethod("empty?")(&q, &[], None).unwrap().inspect_string(),
            "true"
        );
    }

    #[test]
    fn closed_empty_queue_pops_nil() {
        let q = queue_new();
        imethod("close")(&q, &[], None).unwrap();
        assert_eq!(
            imethod("closed?")(&q, &[], None).unwrap().inspect_string(),
            "true"
        );
        assert_eq!(
            imethod("pop")(&q, &[], None).unwrap().inspect_string(),
            "nil"
        );
    }
}
