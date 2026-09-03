# `defined?` must answer `nil` for a call chain that cannot run, never raise.
# zeo evaluates the receiver chain and lets the `NoMethodError` escape.
#
# Ruby evaluates a method-call `defined?` by resolving each link and stopping at
# the first miss (`defined?`'s `NODE_CALL` case walks the receiver, then probes
# the method table without invoking). zeo emits the chain and guards only the
# LAST send, so an intermediate miss escapes as an ordinary raise.
#
# This is the idiom's whole point -- `defined?(a.b.c)` is how ruby code asks
# "is this reachable?" without a rescue -- so a raise here defeats the guard the
# programmer wrote.

# The single-link forms already answer correctly.
p defined?([].size)
p defined?(String.nope)
p defined?([].nope)

# ...and the chain raises rather than answering nil.
p defined?([].nope.nope)
p defined?(nil.foo.bar)
__END__
"method"
nil
nil
nil
nil
