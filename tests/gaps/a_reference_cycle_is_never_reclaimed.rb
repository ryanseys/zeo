# Two objects that point at each other are never reclaimed. zeo's runtime is
# reference counted (`Arc`), so a cycle's counts never reach zero.
#
# This is the README's standing GC limit, filed here so it fails until the
# collector lands rather than living only in prose. It is G14b.
#
# The shape matters: a FINALIZER does not show it, because zeo runs
# finalizers for everything still alive at exit, so `define_finalizer` on a
# cycle prints at exit on both sides and the two agree. A weak reference is
# the honest probe -- ruby's mark-and-sweep clears the entry at `GC.start`,
# zeo's `WeakMap` keeps it because the object is genuinely still alive.
#
# What the collector will and will not reach, so this file is not read as a
# promise of more than is planned: cycles among Objects, Arrays, Hashes,
# Structs, Ranges, Exceptions and compiled Procs are collectable by a
# root-free trial-deletion pass (every reference compiled code holds is a
# real `Arc` strong count, which is exactly that algorithm's input).
# Fibers, Enumerators, Threads, Ractors, class-rooted `define_method` bodies
# and any `RProc` built from a Rust closure are structurally uncollectable --
# the first four hold state on native stacks or another OS thread, and a
# `ClassId` is a `Copy` u32 that is never freed.
#
# Oracle: the cycle is gone after a full mark and sweep.
class Node
  attr_accessor :peer
end
seen = ObjectSpace::WeakMap.new
def build(seen)
  a = Node.new
  b = Node.new
  a.peer = b
  b.peer = a
  seen[a] = true
  nil
end
build(seen)
GC.start(full_mark: true, immediate_sweep: true)
GC.start(full_mark: true, immediate_sweep: true)
p seen.size
