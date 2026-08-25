//! `Process::Waiter` -- what `Process.detach` answers: a REAL `Thread`
//! (`#value` joins the watcher and yields the `Process::Status`) whose class
//! CRuby retags after creation. One own method, because ruby declares
//! exactly one: `#pid`, read from the thread-local the detach stored.

use crate::RubyValue;
use zeo_macros::ruby_class;

ruby_class! {
    Waiter = zeo_abi::PROCESS_WAITER_CLASS < zeo_abi::THREAD_CLASS;

    def "pid"(recv) {
        let RubyValue::Thread(t) = recv else {
            return Ok(RubyValue::Nil);
        };
        Ok(crate::thread::thread_local_get(
            t,
            crate::Symbol::intern("pid"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class declares exactly one own row, and it degrades to nil for a
    /// receiver that is not a Thread -- the whole shape, pinned.
    #[test]
    fn the_one_row_is_pid_and_it_degrades_to_nil() {
        let lookup = crate::builtins::class_table(zeo_abi::PROCESS_WAITER_CLASS)
            .expect("Waiter has a table");
        let pid = lookup("pid").expect("the pid row exists");
        assert!(matches!(
            pid(&RubyValue::Nil, &[], None),
            Ok(RubyValue::Nil)
        ));
    }
}
