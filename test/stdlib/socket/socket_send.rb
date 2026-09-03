# `BasicSocket#send` overrides `Kernel#send`: on a socket, `sock.send(data,
# flags)` writes bytes to the peer and answers the count -- it does NOT invoke a
# method named by `data`. zeo implements `BasicSocket#send`, but `send` is also a
# compiler/runtime intrinsic that reinterprets its first argument as a method
# name whenever the receiver's socket type isn't statically known (here `a` is an
# untyped local from a multiple-assignment), so zeo raises NoMethodError instead
# of sending. This is the same limitation as any redefined `#send` (cf. the
# `Ractor#send` shadow-arm note in `codegen::call`): resolving it needs the
# receiver typed as a `BasicSocket` descendant, which local-type tracking does
# not yet propagate through `a, b = UNIXSocket.pair`.
require "socket"

a, b = UNIXSocket.pair
n = a.send("hello", 0)
puts n
puts b.recv(5)
a.close
b.close
__END__
5
hello
