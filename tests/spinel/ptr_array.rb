# Pointer-array (`<obj>_ptr_array`) coverage — typed-obj `[]` map,
# value-class instances in array literals (and via `push`/`<<`),
# and slot widening from `[nil] * N` writes. Was four tests; the
# only class-name collisions were Foo (same definition reused
# across the two value-class sections — kept once) and Bag (two
# unrelated shapes — section 1's Bag renamed to MapBag).

# === Section 1: ptr_array map ===
# `compile_map_expr` had no `ptr_array` recv branch — `.map { ... }`
# on a typed `obj_X[]` ivar/local fell through to the trailing
# `"0"` placeholder.
class Box
  attr_reader :v
  def initialize(v); @v = v; end
end

class MapBag
  def initialize
    @items = [Box.new(1), Box.new(2), Box.new(3)]
  end
  def doubled
    @items.map { |b| b.v * 2 }
  end
  def names
    @items.map { |b| "v=#{b.v}" }
  end
end

mbag = MapBag.new
mints = mbag.doubled
puts mints.length    # 3
puts mints[0]        # 2
puts mints[2]        # 6
mnames = mbag.names
puts mnames.length   # 3
puts mnames[0]       # v=1
puts mnames[2]       # v=3

# === Section 2 + 3: value-class instances in ptr_array (literal + push) ===
# A class with no attr_writers / no mutating methods would normally
# be value-type'd, but its instances landing in an array literal or
# being push'd into one must demote it to a heap-allocated obj so
# `sp_PtrArray_push`'s `void *` arg type-checks. PR #87 covered the
# literal case; #91 (follow-up) extended it to push / <<.
class Foo
  def initialize(x); @x = x; end
  attr_reader :x
end

# Literal form.
flit = [Foo.new(1), Foo.new(2), Foo.new(3)]
puts flit.length
flit.each { |f| puts f.x }

# `push` form.
fpush = []
fpush.push(Foo.new(1))
fpush.push(Foo.new(2))
puts fpush.length
fpush.each { |f| puts f.x }

# `<<` form (same code path through the parser).
fsh = []
fsh << Foo.new(10)
fsh << Foo.new(20)
fsh << Foo.new(30)
puts fsh.length
fsh.each { |f| puts f.x }

# === Section 4: ptr_array slot widening from [nil] * N ===
# `@arr = [nil] * N` followed by `@arr[i] = obj` widens the slot
# to `<obj>_ptr_array`. Without the widening, the slot stays at
# the default IntArray and the object pointer write is truncated.
class Item
  def initialize(label)
    @label = label
    @history = [0]   # array ivar keeps Item off the value-type fast path
  end
  def visit
    @history << @history.last + 1
    @label
  end
  def visits
    @history.length - 1
  end
end

class Bag
  def initialize
    @slots = [nil] * 4
    @slots[0] = Item.new("a")
    @slots[1] = Item.new("b")
    @slots[3] = Item.new("d")
  end
  attr_reader :slots
  def visit_at(i)
    @slots[i].visit
  end
end

b = Bag.new
puts b.visit_at(0)
puts b.visit_at(1)
puts b.visit_at(3)
puts b.slots[0].visits   # 1
puts b.slots[1].visits   # 1
puts b.slots[3].visits   # 1
