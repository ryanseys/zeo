# Three shapes the cycle collector cannot reclaim. Each is a different cause,
# and none of them is a bug in the reconciliation pass itself -- the pass is
# exact about what it can see, and these are three things it cannot.
#
# The companion `tests/a_cycle_is_reclaimed.rb` is what DOES work. Read that
# first; this file is its boundary.
#
# `proc_cell` used to be the fourth and is now reclaimed, so it lives there
# instead. It is kept HERE as the control: it is the shape closest to the
# three below, and a regression in the proc channel would show up as this
# row answering 1 again rather than as a silent loss of coverage.
#
# 1. A RANGE. `RangeData` is immutable by design (a Range is frozen in ruby
#    and the `Arc` buys sharing with no interior mutability at all), so the
#    sweep has nothing to release its endpoints through. A Proc is immutable
#    too and reports its captures anyway, because every owner of a reclaimed
#    proc is itself reclaimed and cleared, so the proc dies with the pass's
#    handles -- a Range is reached from an ARRAY that the sweep empties, and
#    the same argument would hold. What it lacks is the enumeration, not the
#    release.
#
# 2. A PER-OBJECT SINGLETON, and 3. AN IVAR ON A BARE VALUE. Both side tables
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
