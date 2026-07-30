# A class reopened under a trailing `if` that reads a local of the ENCLOSING
# scope. Zeo runs a class body at its document position, inside the enclosing
# scope's own Rust block, but built that body's context from the body's own
# statements alone -- so the guard, which the site carries in with it, read the
# outer local as an ordinary value while the enclosing scope was keeping it in
# a shared cell. The two disagreed and the program did not compile.
#
# pp.rb is written this way -- `class Set ... end if set_pp`, where `set_pp` is
# a top-level local decided a few lines above -- and it only bites once
# something else in the program makes that scope hold its locals by reference.
#
# Only a TAKEN guard is exercised here. A `def` inside a reopen whose guard is
# false still registers in zeo -- dispatch-table registration is hoisted and
# only the body's EXECUTION is guarded -- which is its own divergence, and not
# the one this covers.
on = true

class String
  def zeo_probe = "string probed"
end if on

p "x".zeo_probe

# The guard is an ordinary expression, not just a bare read.
label = "yes"

class Symbol
  def zeo_probe = "symbol probed"
end if label == "yes"

p :s.zeo_probe

# A class body's OWN local shadows the enclosing one rather than sharing it.
shadowed = "outer"

class Array
  shadowed = "inner"
  def zeo_probe = "array probed"
end if on

p shadowed
p [].zeo_probe

# What makes the enclosing scope keep its locals by reference in the first
# place.
p proc { 1 }.binding.local_variables.include?(:on)
