//! `socket` (CRuby's C `socket` extension). **Scaffolded** -- `require
//! "socket"` activates the `Socket` constant so downstream code compiles past
//! the require, but the networking surface is not yet implemented (`todo!()`
//! markers). A real implementation would wrap `libc` sockets (or `std::net`)
//! and add the `BasicSocket`/`TCPSocket`/`UDPSocket` hierarchy; see
//! docs/EXTENSIONS.md.

use crate::builtins::builtin_methods;

builtin_methods! {
    pub(crate) fn lookup;

    "connect" => fn connect(_recv, _args, _block) { todo!("Socket#connect -- see docs/EXTENSIONS.md") }
    "bind" => fn bind(_recv, _args, _block) { todo!("Socket#bind") }
    "send" => fn send(_recv, _args, _block) { todo!("Socket#send") }
    "recv" => fn recv(_recv, _args, _block) { todo!("Socket#recv") }
    "close" => fn close(_recv, _args, _block) { todo!("Socket#close") }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" | "open" => fn new_m(_recv, _args, _block) { todo!("Socket.new -- see docs/EXTENSIONS.md") }
    "gethostname" => fn gethostname(_recv, _args, _block) { todo!("Socket.gethostname") }
    "getaddrinfo" => fn getaddrinfo(_recv, _args, _block) { todo!("Socket.getaddrinfo") }
    "pair" => fn pair(_recv, _args, _block) { todo!("Socket.pair") }
}
