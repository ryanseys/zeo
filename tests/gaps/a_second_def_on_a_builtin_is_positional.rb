# A second `def` of the same name on a MODULE is not positional. The class
# case is fixed -- `class Array` and `class String` below both answer their
# first body in the window between the two reopens, and ruby's answer for
# the whole Array and String halves matches. What is left is `Kernel`.
#
# WHY THE MODULE IS DIFFERENT, measured 2026-08-26. `analyze::redefs` now
# admits a builtin, splices a `MethodRedefine` for EVERY body (a builtin's
# first body must not install at boot -- the reopen flag keeps the native
# row answering until the reopen stands), and the install runs at exactly
# the right position. The class case works because a value receiver's walk
# probes the overlay per ancestor before that ancestor's registered row.
#
# A MODULE's rows are MATERIALIZED onto every class that includes it: `def
# helper` in `module Kernel` emits `Object#helper` and 29 more copies, one
# per including class. The walk reaches `Object`'s own copy before it ever
# probes `Kernel`, so the overlay row the install writes is never read.
#
# The fix shape: a runtime replacement on a MODULE has to reach the
# materialized copies, the way `refresh_extended_copies` already reaches
# the rows a per-object `extend` copied. It cannot simply overwrite every
# descendant -- a class with its OWN `def helper` keeps its own body, so
# the refresh must know which copies came from THIS module.
#
# Ruby's answer: each body answers between its own reopen and the next.

class Array
  def size = 1
end
p [1, 2].size
class Array
  def size = 2
end
p [1, 2].size

module Kernel
  def helper = "first"
end
puts helper
module Kernel
  def helper = "second"
end
puts helper

class String
  def shout = "first"
end
puts "a".shout
class String
  def shout = "second"
end
puts "a".shout
