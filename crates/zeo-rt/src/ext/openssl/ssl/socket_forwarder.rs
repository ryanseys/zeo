//! `OpenSSL::SSL::SocketForwarder` -- the descriptor-level questions an
//! `SSLSocket` passes down to the socket underneath.
//!
//! Every method here answers about the SOCKET, not the TLS session, so each
//! one forwards to `to_io`. That is upstream's own implementation, and it is
//! why these belong to a module rather than to `SSLSocket`: any object that
//! answers `to_io` can mix them in.

use crate::builtins::arity;
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

/// Send `name` to the receiver's underlying IO. `to_io` rather than a
/// downcast, so the module stays usable by any includer.
fn forward(recv: &RubyValue, name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let io = crate::dispatch::send_value_in(0, recv, crate::Symbol::intern("to_io"), &[], None)?;
    crate::dispatch::send_value_in(0, &io, crate::Symbol::intern(name), args, None)
}

ruby_module! {
    SocketForwarder = zeo_abi::OPENSSL_SOCKET_FORWARDER_MODULE;

    def "closed?" (recv) {
        forward(recv, "closed?", &[])
    }
    def "fileno" (recv) {
        forward(recv, "fileno", &[])
    }
    def "addr" (recv) {
        forward(recv, "addr", &[])
    }
    def "peeraddr" (recv) {
        forward(recv, "peeraddr", &[])
    }
    def "local_address" (recv) {
        forward(recv, "local_address", &[])
    }
    def "remote_address" (recv) {
        forward(recv, "remote_address", &[])
    }
    def "setsockopt" (recv, *args, &_block) {
        arity!(args, 3);
        forward(recv, "setsockopt", args)
    }
    def "getsockopt" (recv, *args, &_block) {
        arity!(args, 2);
        forward(recv, "getsockopt", args)
    }
    def "fcntl" (recv, *args, &_block) {
        forward(recv, "fcntl", args)
    }
    def "close_on_exec=" (recv, *args, &_block) {
        arity!(args, 1);
        forward(recv, "close_on_exec=", args)
    }
    def "close_on_exec?" (recv) {
        forward(recv, "close_on_exec?", &[])
    }
    def "do_not_reverse_lookup=" (recv, *args, &_block) {
        arity!(args, 1);
        forward(recv, "do_not_reverse_lookup=", args)
    }
    def "timeout" (recv) {
        forward(recv, "timeout", &[])
    }
    def "timeout=" (recv, *args, &_block) {
        arity!(args, 1);
        forward(recv, "timeout=", args)
    }
    def "wait" (recv, *args, &_block) {
        forward(recv, "wait", args)
    }
    def "wait_readable" (recv, *args, &_block) {
        forward(recv, "wait_readable", args)
    }
    def "wait_writable" (recv, *args, &_block) {
        forward(recv, "wait_writable", args)
    }
}
