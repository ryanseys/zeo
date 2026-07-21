# trap / Signal.trap / ::Signal.trap compile to no-ops at every shape;
# Spinel has no signal-handler runtime, so the block body (if any)
# never fires. Expression position returns "DEFAULT" -- CRuby's value
# for any signal that was never previously trapped.
#
# Each section uses a distinct signal name so no signal's state is
# observed twice (CRuby would return the prior handler on the second
# touch, which Spinel does not yet model).

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
