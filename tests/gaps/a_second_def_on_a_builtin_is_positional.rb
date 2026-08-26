# A SECOND `def` of the same name on a builtin class or module is not
# positional: the last body answers from program start, so a call written
# between the two reopens gets the wrong one. Wrong values, clean exit.
#
# The first `def` over a builtin already IS positional --
# `tests/a_later_def_on_a_builtin_reaches_back.rb` records that mechanism:
# one `.bss` byte per `(builtin class, method name)` in `zeo_reopen_flags`,
# stored where the reopen stands, read by a prologue that forwards to the
# native row it replaced while the byte is zero. Only ONE flag exists per
# pair, and only the LAST body reaches the static tables, so the first
# user body has nowhere to live.
#
# The shape of the fix: make the byte a COUNT rather than a boolean. Each
# reopen stores its own 1-based ordinal at its position; body `k` runs when
# the count is above `k` and otherwise forwards to body `k-1` (to the
# native row for `k == 0`), which means the superseded bodies have to be
# emitted as well. `analyze::redefs` already knows how to find them --
# `compiler.scopes` is append-only -- but it skips `is_builtin` classes,
# and its `counts >= 2` test is the right one to reuse.
#
# `analyze::redefs`' own mechanism (a runtime overlay install per position)
# is the wrong one to borrow here: it calls `mark_live()`, the global gate
# that takes every send in the program onto the per-ancestor walk. The
# reopen-flag design exists precisely to avoid that.

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
