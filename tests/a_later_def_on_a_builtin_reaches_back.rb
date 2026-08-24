# A later `def` reopening a BUILTIN class used to reach BACK: a call written
# above the reopen answered with the reopened body.
#
# The reopen's row registers at STARTUP, so EVERY site above it was affected --
# statically bound and dynamic alike, and every fold with it. `p [1,2].size`
# before `class Array; def size` answered `:own_size`, and so did a site that
# reached the row through full dispatch. (The gap's original note said both
# calls "bind STATICALLY to `__bm_Array::take_while`"; that was the rustc
# emitter. Under CLIF nothing folds past a reopen, which is what makes the
# whole class of divergence one fix rather than a per-site one.)
#
# The reopen is now POSITIONAL, and both ends are emitted: one `.bss` byte per
# `(builtin class, method name)` in `zeo_reopen_flags`, stored where the
# `class Foo ... end` stands, read by a prologue in the reopened body. Above
# that line the body forwards to the row it replaced. No table registers, the
# runtime looks nothing up, and the inline caches stay valid -- a positional
# OVERLAY install would have called `mark_live()`, the global gate that takes
# every send in the program onto the per-ancestor walk.
#
# Two things the forward is NOT. It is not `super`: `super` resumes PAST the
# reopened class, and a name the builtin owns outright (`Array#size`) has
# nothing above it -- ruby raises "no superclass method" for a `super` written
# there, and rightly. And it is not free of the block: a body that names no
# `&blk` still has to forward the caller's, so a flagged body takes a block
# parameter whether or not it declares one.
#
# `a_reopened_builtin_answers_from_its_own_line.rb` carries the widened sweep.

p [1, 2].take_while { true }
class Array
  def take_while
    :array_own
  end
end
p [1, 2].take_while { true }
