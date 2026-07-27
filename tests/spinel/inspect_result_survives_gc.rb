# An #inspect built in a temporary string builder must hand back a string of
# its own, not the builder's buffer.
#
# The builders are sp_String objects; their buffer carries the 0xfd marker,
# which tells sp_gc_mark "the owner holds this, skip it". Once the helper
# returns, nothing holds the owner: the builder is unreachable, the next
# collection frees it, and the `const char *` the caller rooted points at freed
# memory. Holding the result across a GC.start read it back.

def churn
  a = []
  300.times { |i| a.push("filler #{i}") }
  a.length
end

def through_gc(s)
  churn
  GC.start
  churn
  GC.start
  s
end

p through_gc("abc".match(/b/).inspect)
p through_gc([[1, 2], [3]].inspect)
p through_gc([["a"], ["b"]].inspect)
p through_gc([[1.5], [2.5]].inspect)
p through_gc([[:a], [:b]].inspect)
p through_gc([[1, "x"], [nil]].inspect)
p through_gc({ 1 => 2, 3 => 4 }.inspect)

# the same result kept in a container rather than a local
box = []
box.push("a1b2".match(/(\d)(\w)/).inspect)
box.push([[7, 8]].inspect)
churn
GC.start
churn
GC.start
p box
