# The collector runs on its own once the live registry has grown enough --
# nothing here calls `GC.start`. Before this, a program that never asked
# collected nothing, however much garbage it built.
#
# The trigger is ARMED at the allocation and FIRED at the next `check_ints`
# safepoint, never at the allocation itself. An allocation can happen inside a
# container's own guard -- growing a Hash while its lock is held -- and
# collecting there hands the pass a locked node it has to read. The checkpoint
# sites hold no guard by construction, which a debug assertion states.
#
# The threshold moves with the heap: it is re-aimed after every pass at what
# survived plus half again, so a program with a genuinely large LIVE heap
# collects on growth rather than every time the registry compacts. Only the
# survivor count a compaction already computed is read, so the trigger costs
# no scan of its own.
#
# An automatic collection takes the SAME entry an explicit `GC.start` does, so
# the two cannot drift: `GC.disable` gates both, both bump `GC.count`, and
# both sweep the finalizers a reclaimed cycle just made due. CRuby counts its
# automatic collections too.
class Node
  attr_accessor :peer
end

def churn(seen = nil)
  a = Node.new
  b = Node.new
  a.peer = b
  b.peer = a
  seen[a] = true if seen
  nil
end

watch = ObjectSpace::WeakMap.new
churn(watch)
before = GC.count
150_000.times { churn }

# The count moved without the program asking, and the watched cycle is gone.
p GC.count > before
p watch.size == 0
