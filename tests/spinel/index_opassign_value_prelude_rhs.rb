# Index operator-assignment in VALUE position whose RIGHT-HAND SIDE needs a
# prelude. `index_opassign_in_thread_block.rb` pinned the value-position
# op-write itself (a block lifted to a real proc, so its tail `arr[i] += x` is
# emitted through the value form). That form writes the mutation into the
# prelude buffer and then reads the slot back -- but while it was writing
# THERE, an RHS wanting a prelude of its own (a user method call hoists its
# receiver and spills each argument into a temp) had nowhere to put it and
# emitted the declaration inline, inside the `sp_..._set(...)` argument list.
# The result was structurally invalid C -- an unbalanced paren and a `_tN` used
# before its declaration -- so it surfaced as a cc failure, not a type error.
#
# A literal or a bare local on the RHS needs no prelude and always worked, and
# so did `arr[i] = arr[i] + <call>`; only the compound form in value position
# with a prelude-carrying RHS broke.
#
# Reported as `arr[i] += <call> if <cond>`, but the trailing `if` is NOT part of
# the trigger -- it only put the op-write in tail position. That spelling emits
# byte-identical code to case 1, as does the same op-write reached through a
# Thread (already covered by index_opassign_in_thread_block.rb), so neither
# earns a case here. What follows is one case per distinct emitted path.

class Outcome
  def map_one(f)
    f + 1.0
  end
end

outcome = Outcome.new

# 1. the reduced engine shape: captured float array, captured object receiver,
#    one argument -- spilled into a prelude temp -- as the body's tail.
a = [1.0]
->() { a[0] += outcome.map_one(2.5) }.call
p a

# 2. the prelude is the RECEIVER hoist rather than an argument spill, and it
#    allocates, so it carries a GC root that must also land before the write.
b = [1.0]
->() { b[0] += Outcome.new.map_one(2.5) }.call
p b

# 3. integer array (a different set/get arm), and the op-write's own value is
#    still the stored (post-op) result.
d = [10]
r = ->() { d[0] += outcome.map_one(7).to_i }.call
p r
p d

# 4. hash receiver -- another arm, with its own key handling.
h = { "k" => 10 }
->() { h["k"] += outcome.map_one(5).to_i }.call
p h

# 5. a method-call KEY is not effect-free, so receiver and key are hoisted into
#    shared temps first (the #3417 path). Those hoisted lines, the RHS's
#    prelude, and the mutation must come out in that order.
class Slot
  def tag
    0
  end
end
v = [1.0]
s = Slot.new
->() { v[s.tag] += outcome.map_one(2.5) }.call
p v
