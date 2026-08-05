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
