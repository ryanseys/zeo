//! `Queue`'s method rows. Every queue call dispatches through these rows,
//! which wrap `crate::thread`'s `queue_push`/`queue_pop`/... primitives
//! with CRuby's exact contract -- `push` to a closed queue is a
//! `ClosedQueueError`, `pop` blocks until an element arrives, a closed
//! empty queue pops `nil`.
//! `Queue` has a dedicated `RubyValue::Queue` variant (like
//! Thread/Fiber), unwrapped with `as_queue_unchecked`. `SizedQueue` (a
//! sibling file) inherits every instance row here through the ancestry walk,
//! since `SizedQueue < Queue`, and adds the bound (`max`/`max=`) that only a
//! bounded queue answers.
use crate::builtins::inherited_row;

use crate::RubyValue;
use crate::thread::{queue_close, queue_closed, queue_len, queue_new, queue_pop, queue_push};
use zeo_macros::ruby_class;

ruby_class! {
    Queue = zeo_abi::QUEUE_CLASS < zeo_abi::OBJECT_CLASS;

    def self."new" cfunc (_recv) {
        Ok(queue_new())
    }

    // `push`/`<<`/`enq` enqueue one value and return self; pushing to a
    // closed queue is a `ClosedQueueError`.
    def "push" | "<<" | "enq" (recv, other) {
        let q = recv.as_queue_unchecked();
        // The signal is PROPAGATED, not re-labelled: `queue_push` can also
        // answer CRuby's `fatal` when a SizedQueue's back-pressure can never
        // be relieved, and swallowing that reported "queue closed" for a
        // queue nobody had closed.
        queue_push(&q, (*other).clone()).map_err(crate::thread::WaitFailure::signal)?;
        Ok(RubyValue::Queue(q))
    }
    // `pop`/`shift`/`deq` -- block (yielding) while empty and open; a closed
    // empty queue pops `nil`.
    def "pop" params "non_block = nil, timeout: nil"
        | "shift" params "non_block = nil, timeout: nil"
        | "deq" params "non_block = nil, timeout: nil" cfunc (recv) {
        queue_pop(&recv.as_queue_unchecked())
    }
    def "close"(recv) {
        let q = recv.as_queue_unchecked();
        queue_close(&q);
        Ok(RubyValue::Queue(q))
    }
    def "closed?"(recv) {
        Ok(RubyValue::Bool(queue_closed(&recv.as_queue_unchecked())))
    }
    def "length" | "size" (recv) {
        Ok(RubyValue::Int(queue_len(&recv.as_queue_unchecked())))
    }
    def "empty?"(recv) {
        Ok(RubyValue::Bool(queue_len(&recv.as_queue_unchecked()) == 0))
    }
    // `clear` drops every queued element and releases any back-pressured
    // pusher; it answers the queue, as every Ruby `clear` does.
    def "clear"(recv) {
        let q = recv.as_queue_unchecked();
        crate::thread::queue_clear(&q);
        Ok(RubyValue::Queue(q))
    }
    // Re-init EMPTIES the queue (oracle-pinned), then seeds from an
    // optional enumerable like `Queue.new(items)`.
    private def "initialize" cfunc (recv, *args) {
        let q = recv.as_queue_unchecked();
        crate::thread::queue_clear(&q);
        if let Some(src) = args.first()
            && !src.is_nil()
        {
            let RubyValue::Array(items) = src else {
                return Err(crate::builtins::type_error!(
                    "can't convert {} into Array",
                    crate::builtins::class_name_of(src)
                ));
            };
            for v in items.lock().iter() {
                queue_push(&q, v.clone()).map_err(crate::thread::WaitFailure::signal)?;
            }
        }
        Ok(recv.clone())
    }
    def "num_waiting"(recv) {
        Ok(RubyValue::Int(crate::thread::queue_num_waiting(&recv.as_queue_unchecked())))
    }
    // A Queue owns a condvar and a parked-thread count, neither of which
    // survives a round trip, so CRuby refuses to dump one. Same for its
    // sibling `ConditionVariable`.
    def "marshal_dump"(recv) {
        Err(crate::builtins::type_error!(
            "can't dump {}", crate::builtins::class_name_of(recv)))
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "freeze"(recv) { inherited_row!(kernel, "freeze", recv, __args, None) }
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
