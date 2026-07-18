//! `socket` (CRuby's C `socket` extension). **Scaffolded** -- `require
//! "socket"` activates the `Socket` constant so downstream code compiles past
//! the require. The networking surface (`connect`/`bind`/`send`/`recv`, the
//! `BasicSocket`/`TCPSocket`/`UDPSocket` hierarchy) needs live OS sockets, which
//! can't be oracle-tested in this harness; rather than a `todo!()` panic, each
//! entry raises a rescue-able `NotImplementedError` so a program probing for the
//! capability degrades gracefully. See docs/EXTENSIONS.md.

use crate::builtins::builtin_methods;
use crate::dispatch::raise_error;
use crate::Signal;

/// The shared "networking not built" error -- a real, `rescue`-able
/// `NotImplementedError` (not a panic).
fn not_implemented(method: &str) -> Signal {
    raise_error(
        "NotImplementedError",
        format!("Socket#{method} is not implemented (live networking is out of scope)"),
    )
}

builtin_methods! {
    pub(crate) fn lookup;

    "connect" => fn connect(_recv, _args, _block) { Err(not_implemented("connect")) }
    "bind" => fn bind(_recv, _args, _block) { Err(not_implemented("bind")) }
    "send" => fn send(_recv, _args, _block) { Err(not_implemented("send")) }
    "recv" => fn recv(_recv, _args, _block) { Err(not_implemented("recv")) }
    "close" => fn close(_recv, _args, _block) { Err(not_implemented("close")) }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" | "open" => fn new_m(_recv, _args, _block) { Err(not_implemented("new")) }
    "gethostname" => fn gethostname(_recv, _args, _block) { Err(not_implemented("gethostname")) }
    "getaddrinfo" => fn getaddrinfo(_recv, _args, _block) { Err(not_implemented("getaddrinfo")) }
    "pair" => fn pair(_recv, _args, _block) { Err(not_implemented("pair")) }
}
