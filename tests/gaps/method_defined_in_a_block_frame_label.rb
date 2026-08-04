# A method DEFINED inside a block carries the block's nesting in its frame
# label, so `caller` and `caller_locations` name it wrongly:
#
#     "block (2 levels) in Object#c1"   instead of   "Object#c1"
#
# The label is baked when codegen emits the method's `FrameGuard`, and for a
# `def` reached through a block it is built from the emitting position rather
# than from the method the `def` creates. Where the `def` was WRITTEN has no
# bearing on what the resulting method is called -- ruby names the frame after
# the method, and a `def` inside a block produces an ordinary method of the
# enclosing class.
#
# It reaches anything that reads a frame by name: a logger tagging its call
# site, a DSL blaming the right line, and every `caller` assertion in a test.
# Defining methods inside `each`/`class_exec`/`instance_eval` blocks is the
# normal shape for a generated API, so the mislabelled frames are the ones a
# generated method produces.

[1].each do
  def c1 = c2
  def c2 = caller_locations(1, 1).first.label
end
p c1

# The same method, called from a block rather than defined in one, is right.
[1].each { p c2 }

# A top-level def is already right.
def d1 = d2
def d2 = caller_locations(1, 1).first.label
p d1
