# `arr.map { Obj.new }` where Obj is a value-type-candidate class (no
# subclass, immutable scalar ivars) used to build-fail: detect_value_types
# never saw the map result as a ptr_array store, so Obj stayed value-typed
# and `sp_Obj_new(...)` returned a struct by value that couldn't be pushed
# into the object accumulator. detect_ptr_array_stored_types now forces a
# `map`-collected class heap. Covers both typed-array and range receivers
# (the range path previously bailed to a silent "0").
class Node
  attr_reader :tag
  def initialize(t = 0); @tag = t; end
end

ks = [1, 2, 3].map { |t| Node.new(t) }
puts ks.size
puts ks[0].tag
puts ks[2].tag
total = 0
ks.each { |n| total += n.tag }
puts total

rs = (1..3).map { |t| Node.new(t * 10) }
puts rs.size
puts rs.map(&:tag).inspect

es = (1...4).map { |t| Node.new(t) }
puts es.map(&:tag).inspect
