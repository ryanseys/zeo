# zeo's heap is reference counted, so a cycle's counts never reach zero and
# nothing in it is ever dropped. The cycle collector reconciles those counts
# against an allocation registry instead of tracing from roots -- zeo has no
# enumerable root set and never will -- and reclaims what nothing outside the
# registry refers to.
#
# `ObjectSpace::WeakMap` is the honest probe. A finalizer is not: zeo runs
# finalizers for everything still alive at exit, so `define_finalizer` on a
# cycle prints at exit on both sides whether or not the cycle was collected.
# A weak reference reports what actually happened -- both implementations
# clear the entry only when the object is really gone.
#
# Every shape is built inside a method that has returned, so no local is
# holding it. The survivors are held on purpose, and are the half that
# matters most: a collector that reclaims a live object is far worse than one
# that misses a dead one.
#
# Four of these arrived from `tests/gaps/a_cycle_can_still_leak.rb`, which is
# gone: a captured proc cell, a Range endpoint, a per-object singleton, and an
# ivar on a bare value. Their causes are recorded beside each one, because
# each was a different mechanism rather than four instances of one bug.
seen = ObjectSpace::WeakMap.new
$held = []

class Node
  attr_accessor :peer, :tag
  def initialize(tag = nil) = @tag = tag
end
class ArraySub < Array; end
class HashSub < Hash; end
class Boom < StandardError
  attr_accessor :peer
end
Pair = Struct.new(:peer)

def pair(seen)
  a = Node.new
  b = Node.new
  a.peer = b
  b.peer = a
  seen[a] = true
  seen[b] = true
  nil
end

def self_loop(seen)
  a = Node.new
  a.peer = a
  seen[a] = true
  nil
end

def chain(seen)
  ns = Array.new(4) { Node.new }
  ns.each_with_index { |n, i| n.peer = ns[(i + 1) % 4] }
  ns.each { |n| seen[n] = true }
  nil
end

def through_array(seen)
  a = []
  a << a
  seen[a] = true
  nil
end

def through_hash(seen)
  h = {}
  h[:self] = h
  k = []
  h[k] = k
  seen[h] = true
  seen[k] = true
  nil
end

def through_containers(seen)
  n = Node.new
  n.peer = { list: [n] }
  seen[n] = true
  nil
end

def through_a_value_subclass(seen)
  a = ArraySub.new
  a << a
  h = HashSub.new
  h[:me] = h
  seen[a] = true
  seen[h] = true
  nil
end

def through_an_exception(seen)
  e = Boom.new("x")
  e.peer = e
  seen[e] = true
  nil
end
# Deliberately absent: a cycle closed through `Exception#cause`. zeo reclaims
# it; CRuby keeps the most recently raised exception reachable from its own VM
# error slot, so the oracle answers one more than zeo does and the shape is
# not portable. The `cause` slot is still enumerated like every other hidden
# one -- the corpus's own raising goldens exercise it under the collector's
# debug over-count check.

def through_a_struct(seen)
  s = Pair.new(nil)
  s.peer = s
  seen[s] = true
  nil
end

def through_an_invented_ivar(seen)
  n = Node.new
  n.instance_variable_set(:@invented, n)
  seen[n] = true
  nil
end

def through_a_string_leaf(seen)
  n = Node.new
  n.peer = ["a#{1}b", n]
  seen[n] = true
  nil
end

# `Object.new` keeps its ivars in a name-keyed map rather than in struct
# fields, because the top level's `self` has no compile-time class whose ivar
# list codegen could have materialized.
def through_a_bare_object(seen)
  o = Object.new
  o.instance_variable_set(:@peer, o)
  seen[o] = true
  nil
end

# A `Struct.new` the compiler cannot see is a run-time instance, whose
# members AND ivars are both mutable owned slots. `Pair` above is the
# compile-time twin, which is a generated class instead.
def through_a_runtime_struct(seen)
  k = Struct.new(:peer)
  s = k.new(nil)
  s.peer = s
  m = Struct.new(:tag).new(1)
  m.instance_variable_set(:@peer, m)
  seen[s] = true
  seen[m] = true
  nil
end

def through_runtime_class(seen)
  k = Class.new { attr_accessor :peer }
  x = k.new
  y = k.new
  x.peer = y
  y.peer = x
  seen[x] = true
  nil
end

# A compiled block's captured cell, which is where `obj.callback = -> { obj }`
# -- the canonical ruby leak -- closes. The environment lives inside an
# `Arc<dyn Fn>` that no reflection can open, so the proc holds a copy of it
# beside the closure purely so the walk can enumerate the cells.
def through_a_captured_cell(seen)
  n = Node.new
  n.peer = -> { n }
  seen[n] = true
  nil
end

# The same cell shared by two procs: reported once by each, which is what the
# over-count check is there to catch if it is ever reported twice by one.
def through_two_procs_sharing_a_cell(seen)
  n = Node.new
  n.peer = [-> { n }, -> { n }]
  seen[n] = true
  nil
end

# A block's lexical `self`, which is a capture the closure does not hold at
# all -- it rides beside the body so `instance_exec` can rebind it.
def through_a_block_self(seen)
  n = Node.new
  n.peer = proc { @anything }
  seen[n] = true
  nil
end

# A Range holds its endpoints with no interior mutability at all -- it is
# frozen in ruby, and the `Arc` buys sharing and object identity, nothing
# else. So it reports edges the sweep cannot clear, exactly as a Proc does.
# Only a Range that COULD be in a cycle is registered: `1..n` owns nothing.
def through_a_range_endpoint(seen)
  a = []
  a << (a..nil)
  seen[a] = true
  nil
end

# A per-object singleton files its methods in a side table keyed by the
# receiver's ADDRESS, which is unique only while the receiver lives. The pin
# that stops a later value inheriting those methods is a WEAK reference: it
# holds the allocation, so the address stays reserved, without holding the
# value.
def through_a_per_object_singleton(seen)
  n = Node.new
  def n.only_mine = 1
  n.peer = n
  seen[n] = true
  nil
end

# An ivar on a bare value is the other table with that shape, and the same
# weak pin.
def through_an_ivar_on_a_bare_value(seen)
  a = []
  a << a
  a.instance_variable_set(:@tag, 1)
  seen[a] = true
  nil
end

# -- the survivors ---------------------------------------------------------

def held_by_a_global(seen)
  a = Node.new
  a.peer = a
  $held << a
  seen[a] = true
  nil
end

def held_by_a_container(seen, keep)
  a = Node.new
  b = Node.new
  a.peer = b
  b.peer = a
  keep << a
  seen[a] = true
  seen[b] = true
  nil
end

# CRuby scans the machine stack conservatively, so a pointer a returned frame
# left behind can keep a dead object alive for a while. Overwriting that stack
# is what makes the oracle's answer deterministic; zeo needs no such help,
# because a reference count is exact.
def scrub(n) = n.zero? ? [0] * 512 : scrub(n - 1)

keep = []
pair(seen)
self_loop(seen)
chain(seen)
through_array(seen)
through_hash(seen)
through_containers(seen)
through_a_value_subclass(seen)
through_an_exception(seen)
through_a_struct(seen)
through_an_invented_ivar(seen)
through_a_string_leaf(seen)
through_runtime_class(seen)
through_a_bare_object(seen)
through_a_runtime_struct(seen)
through_a_captured_cell(seen)
through_two_procs_sharing_a_cell(seen)
through_a_block_self(seen)
through_a_range_endpoint(seen)
through_a_per_object_singleton(seen)
through_an_ivar_on_a_bare_value(seen)
held_by_a_global(seen)
held_by_a_container(seen, keep)

scrub(60)
GC.start
GC.start

# 1 global cycle + 2 objects reached from `keep`
p seen.size
p $held.size
p keep.size
p keep[0].peer.peer.equal?(keep[0])
p $held[0].peer.equal?($held[0])

# The survivors are intact, not merely present: a collector that emptied a
# live object would show up here rather than as a crash.
live = Node.new("live")
live.peer = live
p live.tag
GC.start
p live.tag
p live.peer.equal?(live)
