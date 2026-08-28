# `_1` .. `_9` (and `it`, which the parser lowers to `_1`) are names spinel
# synthesizes rather than names the author wrote, and blocks share their
# enclosing scope's local table -- so two such blocks in one method interned
# the SAME slot and their types merged. Values stayed right, because each block
# writes the slot before reading it; what it cost was the type.
#
# A scope with one numbered-param block over integers gets `sp_int lv__1`. Add
# a second over strings and, before this, BOTH became a boxed sp_RbVal. It also
# left two passes typing that one slot from different call shapes on every
# round, which was the last program in the suite whose inference fixpoint ran
# to its 128-round cap (#4116).
def alone
  out = []
  [10, 20].each { out << _1 + 1 }
  out
end

def together
  seen = []
  [1, 2].each { seen << _1 }
  ["a", "b"].each { seen << _1 }
  seen
end

p alone
p together

# Three blocks, three element types, all in one scope.
def three
  a = []
  [1, 2].each { a << _1 * 2 }
  ["x"].each { a << _1 * 2 }
  [1.5].each { a << _1 * 2 }
  a
end
p three

# `it` is the same slot by another spelling.
def with_it
  r = []
  [1, 2].each { r << it }
  ["p", "q"].each { r << it }
  r
end
p with_it

# _2 as well as _1, and a block that uses only _2.
def pairs
  out = []
  { a: 1, b: 2 }.each { out << [_1, _2] }
  [[3, 4], [5, 6]].each { out << _2 }
  out
end
p pairs

# The first-class forms keep working: a proc/lambda's numbered parameters bind
# from the argument channel, and Proc#parameters and #arity report the names
# Ruby reports whatever the slot is called internally.
f = -> { _1 * 10 }
p f.call(5)
p f.arity
g = proc { _1 + _2 }
p g.call(1, 2)
p proc { _1 }.parameters
p lambda { _1 }.call("a")
p(-> { _1 }.call("a"))

# A block nested inside a method that also has one: different scopes, so no
# collision and no rename.
def outer
  x = [1, 2].map { _1 + 1 }
  x
end
def other
  y = ["a"].map { _1 }
  y
end
p [outer, other]
