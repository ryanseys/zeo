# trap / Signal.trap / ::Signal.trap at every shape. No signal is delivered,
# so no handler body runs; in expression position the call answers "DEFAULT",
# the value for a signal that was never trapped before.
#
# Each section uses a distinct signal name, so no signal's state is observed
# twice: a second trap of the same name answers the handler set by the first.

# Stmt position, implicit-self.
trap("INT") { puts "handler" }

# Stmt position, explicit Signal receiver (ConstantReadNode).
Signal.trap("TERM") { puts "handler" }

# Stmt position, toplevel ::Signal receiver (ConstantPathNode).
::Signal.trap("HUP") { puts "handler" }

# Stmt position, no block.
trap("QUIT", "EXIT")

# Expr position, first call on a never-trapped signal returns "DEFAULT".
prev = trap("USR1") { puts "x" }
puts prev

# Expr position via Signal receiver, also returns "DEFAULT" on first call.
prev2 = Signal.trap("USR2") { puts "y" }
puts prev2

puts "done"
__END__
DEFAULT
DEFAULT
done
