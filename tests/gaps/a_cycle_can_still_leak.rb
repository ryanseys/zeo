# Four shapes the cycle collector cannot reclaim. Each is a different cause,
# and none of them is a bug in the reconciliation pass itself -- the pass is
# exact about what it can see, and these are four things it cannot.
#
# The companion `tests/a_cycle_is_reclaimed.rb` is what DOES work. Read that
# first; this file is its boundary.
#
# 1. A CELL A COMPILED PROC CAPTURED. `n.peer = -> { n }` closes through the
#    cell the block and its enclosing scope share. The cell is a registered
#    node, but its owner is the `ProcEnvOwned` inside an `Arc<dyn Fn>`, which
#    no enumeration can reach -- so the cell keeps an owner the walk cannot
#    account for and everything it reaches stays live. Registering a proc
#    means holding the env beside the closure instead of inside it, AND
#    unpicking the `with_*` builder chain: those eight builders are
#    `Arc::get_mut` on a refcount-1 handle, and one weak handle taken at
#    construction silently turns every one of them into a no-op.
#
# 2. A RANGE. `RangeData` is immutable by design (a Range is frozen in ruby
#    and the `Arc` buys sharing with no interior mutability at all), so the
#    sweep has nothing to release its endpoints through. The rule the
#    collector is built on is that a type reporting an edge must be able to
#    release it -- reporting one it cannot clear would let the pass reclaim a
#    node that edge still points at -- so a Range reports nothing and a cycle
#    through one survives.
#
# 3. A PER-OBJECT SINGLETON, and 4. AN IVAR ON A BARE VALUE. Both side tables
#    are keyed by the owner's ADDRESS and hold a STRONG reference to it, so a
#    dead value's address can never be handed to an unrelated later one that
#    would inherit its rows. The pin makes the owner permanently live, which
#    the walk correctly reads as "referenced from outside the registry".
#    A `Weak` would serve the same purpose -- it pins the ALLOCATION, so the
#    address stays unique, while `upgrade` reports the value is gone and the
#    row can be evicted. That is the fix, and it is its own pass over both
#    tables.
#
# Oracle: every one of them is collected.
class Node
  attr_accessor :peer
end

def scrub(n) = n.zero? ? [0] * 512 : scrub(n - 1)

def proc_cell(seen)
  n = Node.new
  n.peer = -> { n }
  seen[n] = true
  nil
end

def range_endpoint(seen)
  a = []
  a << (a..nil)
  seen[a] = true
  nil
end

def per_object_singleton(seen)
  n = Node.new
  def n.only_mine = 1
  n.peer = n
  seen[n] = true
  nil
end

def ivar_on_a_bare_value(seen)
  a = []
  a << a
  a.instance_variable_set(:@tag, 1)
  seen[a] = true
  nil
end

%i[proc_cell range_endpoint per_object_singleton ivar_on_a_bare_value].each do |name|
  seen = ObjectSpace::WeakMap.new
  send(name, seen)
  scrub(60)
  GC.start
  GC.start
  puts "#{name}\t#{seen.size}"
end
