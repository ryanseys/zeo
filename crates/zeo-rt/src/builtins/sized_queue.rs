//! `SizedQueue < Queue` -- a bounded blocking queue. Its only own method is
//! the bounded constructor `SizedQueue.new(n)`; every instance row (`push`/
//! `pop`/`max`/...) is inherited from `Queue` through the ancestry walk, since
//! `SizedQueue`'s superclass is `Queue`. Instances are the same
//! `RubyValue::Queue` values, whose runtime payload carries the bound.

use crate::builtins::arg_int;
use crate::thread::sized_queue_new;
use zeo_macros::ruby_class;

ruby_class! {
    SizedQueue = zeo_abi::SIZED_QUEUE_CLASS < zeo_abi::QUEUE_CLASS;

    // `SizedQueue.new(n)` -- the bounded constructor.
    def self."new" cfunc (_recv, arg) {
        Ok(sized_queue_new(arg_int!(arg)))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sized_queue_registers_its_bounded_constructor() {
        let t = crate::builtins::registered_table(zeo_abi::SIZED_QUEUE_CLASS)
            .expect("SizedQueue is a registered builtin table");
        assert!((t.class.as_ref().unwrap().lookup)("new").is_some());
        // It carries no instance table of its own -- Queue's rows are inherited
        // through the ancestry walk (`SizedQueue < Queue`).
        assert!(t.instance.is_none());
    }
}
