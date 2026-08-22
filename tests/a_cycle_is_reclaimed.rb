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
seen = ObjectSpace::WeakMap.new
$held = []

class Node
  attr_accessor :peer, :tag
  def initialize(tag = nil) = @tag = tag
end

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

def through_runtime_class(seen)
  k = Class.new { attr_accessor :peer }
  x = k.new
  y = k.new
  x.peer = y
  y.peer = x
  seen[x] = true
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

keep = []
pair(seen)
self_loop(seen)
chain(seen)
through_array(seen)
through_hash(seen)
through_containers(seen)
through_runtime_class(seen)
held_by_a_global(seen)
held_by_a_container(seen, keep)

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
