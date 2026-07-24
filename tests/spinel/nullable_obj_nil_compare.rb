# Issue #423. `if c != nil` (or `while c != nil`) on a nullable
# user-class local emitted the body unconditionally -- the `!=
# nil` comparison was constant-folded to TRUE, the surrounding
# `if` was collapsed (compile_if_stmt's "static TRUE fold"), and
# the body ran even when c was NULL. Found in tep's route
# dispatch loop: `while route != nil && !served` entered with
# route = NULL after a no-match POST and segfaulted.
#
# Root cause: `compile_eq`'s cross-type fold treated `obj_<C>? ==
# nil` and `obj_<C>? != nil` as type-strict pairs (LHS is obj,
# RHS is nil-typed; the existing fold returned "FALSE" / "TRUE"
# because "obj vs nil are different types, always unequal").
# That's right for a non-nullable `obj_<C>` -- the C pointer is
# never NULL by design -- but wrong for `obj_<C>?` where NULL is
# the runtime nil sentinel.
#
# Fix: in compile_eq's cross-type-prim-vs-obj branch, when
# other_t == "nil", check is_nullable_type on the obj side. If
# nullable, emit the actual pointer comparison (`obj == NULL` /
# `obj != NULL`); fall through to the original FALSE/TRUE fold
# only for non-nullable obj types.
#
# Coverage:
#   - if c != nil / if c / if !c.nil? on a function returning
#     nil-or-Foo: all three forms agree.
#   - while c != nil on the same shape: loop terminates instead
#     of spinning forever.
#   - non-nullable obj_<C> (c = Foo.new) still folds to TRUE for
#     != nil (no pointer-vs-NULL emit needed; the value is never
#     NULL).

class Box
  attr_accessor :tag
  def initialize(tag)
    @tag = tag
  end
end

def find_or_nil(want)
  if want == "yes"
    return Box.new("found")
  end
  nil
end

# 1. nil branch -- all three forms should agree.
c = find_or_nil("no")
puts "c=" + (c.nil? ? "nil" : "non-nil")     # c=nil

if c != nil
  puts "ne-nil: ENTERED"
else
  puts "ne-nil: skipped"                      # skipped (correct)
end

if c
  puts "tr: ENTERED"
else
  puts "tr: skipped"                          # skipped
end

if !c.nil?
  puts "not-nilq: ENTERED"
else
  puts "not-nilq: skipped"                    # skipped
end

# 2. non-nil branch -- all three forms enter the body.
d = find_or_nil("yes")
puts "d=" + (d.nil? ? "nil" : "non-nil")     # d=non-nil

if d != nil
  puts "d-ne-nil: ENTERED"                    # ENTERED
else
  puts "d-ne-nil: skipped"
end

# 3. while loop terminates instead of spinning.
n = 0
while c != nil
  n += 1
  break if n > 3
end
puts "n=" + n.to_s                            # n=0

# 4. non-nullable obj (Foo.new is always non-nil) still folds.
e = Box.new("eager")
if e != nil
  puts "e-ne-nil: ENTERED"                    # ENTERED (non-nullable, always true)
else
  puts "e-ne-nil: skipped"
end
