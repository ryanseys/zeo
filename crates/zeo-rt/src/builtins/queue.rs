//! `Queue`'s Path-2 (runtime `send`) rows -- for a dynamically-typed Queue
//! receiver (a queue held in an Array/Hash/ivar, or a `Poly` `send` target).
//! The static Path-1 codegen arm (`codegen::call`) emits `queue_push`/
//! `queue_pop`/... directly and never reaches here; these rows mirror it
//! exactly -- `push` to a closed queue is a `ClosedQueueError`, `pop` blocks
//! coroutine-yieldingly, a closed empty queue pops `nil` -- so both paths
//! agree. `Queue` has a dedicated `RubyValue::Queue` variant (like
//! Thread/Fiber), unwrapped with `as_queue_unchecked`.

use crate::RubyValue;
use crate::builtins::{arg_int, arity, builtin_methods};
use crate::dispatch::raise_error;
use crate::thread::{
    queue_close, queue_closed, queue_len, queue_max, queue_new, queue_pop, queue_push,
    queue_set_max, sized_queue_new,
};

builtin_methods! {
    pub(crate) fn lookup;

    // `push`/`<<`/`enq` enqueue one value and return self; pushing to a
    // closed queue is a `ClosedQueueError`.
    "push" | "<<" | "enq" => fn push(recv, args, _block) {
        arity!(args, 1);
        let q = recv.as_queue_unchecked();
        match queue_push(&q, args[0].clone()) {
            Ok(()) => Ok(RubyValue::Queue(q)),
            Err(_) => Err(raise_error("ClosedQueueError", "queue closed".to_string())),
        }
    }
    // `pop`/`shift`/`deq` -- block (yielding) while empty and open; a closed
    // empty queue pops `nil`.
    "pop" | "shift" | "deq" => fn pop(recv, args, _block) {
        arity!(args, 0);
        queue_pop(&recv.as_queue_unchecked())
    }
    "close" => fn close(recv, args, _block) {
        arity!(args, 0);
        let q = recv.as_queue_unchecked();
        queue_close(&q);
        Ok(RubyValue::Queue(q))
    }
    "closed?" => fn closed_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(queue_closed(&recv.as_queue_unchecked())))
    }
    "length" | "size" => fn length(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(queue_len(&recv.as_queue_unchecked())))
    }
    "empty?" => fn empty_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(queue_len(&recv.as_queue_unchecked()) == 0))
    }
    // `SizedQueue#max`/`max=` -- the bound. `max` on an unbounded `Queue`
    // answers `nil` (a documented divergence: CRuby has no `Queue#max`).
    "max" => fn max(recv, args, _block) {
        arity!(args, 0);
        Ok(match queue_max(&recv.as_queue_unchecked()) {
            Some(n) => RubyValue::Int(n),
            None => RubyValue::Nil,
        })
    }
    "max=" => fn set_max(recv, args, _block) {
        arity!(args, 1);
        queue_set_max(&recv.as_queue_unchecked(), arg_int!(args, 0));
        Ok(args[0].clone())
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" => fn new_m(_recv, args, _block) {
        arity!(args, 0);
        Ok(queue_new())
    }
}

builtin_methods! {
    pub(crate) fn lookup_class_sized;

    // `SizedQueue.new(n)` -- the bounded constructor.
    "new" => fn sized_new(_recv, args, _block) {
        arity!(args, 1);
        Ok(sized_queue_new(arg_int!(args, 0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_the_dynamic_queue_surface() {
        assert!(lookup("push").is_some());
        assert!(lookup("<<").is_some());
        assert!(lookup("pop").is_some());
        assert!(lookup("nope").is_none());
        assert!(lookup_class("new").is_some());
    }

    #[test]
    fn push_len_pop_empty_round_trip() {
        let q = queue_new();
        assert_eq!(empty_p(&q, &[], None).unwrap().inspect_string(), "true");
        push(&q, &[RubyValue::Int(1)], None).unwrap();
        push(&q, &[RubyValue::Int(2)], None).unwrap();
        assert_eq!(length(&q, &[], None).unwrap().inspect_string(), "2");
        assert_eq!(pop(&q, &[], None).unwrap().inspect_string(), "1");
        assert_eq!(pop(&q, &[], None).unwrap().inspect_string(), "2");
        assert_eq!(empty_p(&q, &[], None).unwrap().inspect_string(), "true");
    }

    #[test]
    fn closed_empty_queue_pops_nil() {
        let q = queue_new();
        close(&q, &[], None).unwrap();
        assert_eq!(closed_p(&q, &[], None).unwrap().inspect_string(), "true");
        assert_eq!(pop(&q, &[], None).unwrap().inspect_string(), "nil");
    }
}
