# Bundled tests:
#   - array_each_with_object
#   - array_plus

# === array_each_with_object ===
# Array#each_with_object on poly_array and ptr_array used to silently
# miss the type-check; the loop body never ran. Both shapes covered.

# poly_array (heterogeneous)
n = 0
[1, "x"].each_with_object("") {|_e, _a| n += 1 }
puts n

# ptr_array (user objects)
class T_array_each_with_object_Bar
  def initialize(x); @x = x; end
  attr_accessor :x
end

m = 0
[T_array_each_with_object_Bar.new(1), T_array_each_with_object_Bar.new(2)].each_with_object("") {|_e, _a| m += 1 }
puts m

# === array_plus ===
# Array#+ used to fall through on poly_array / ptr_array — the result
# temp held its default 0 because the dispatcher's type-list omitted
# those shapes.

# poly_array
a = [1, "x"]
b = [2, "y"]
puts (a + b).length

# ptr_array
class T_array_plus_Bar
  def initialize(x); @x = x; end
  attr_accessor :x
end

c = [T_array_plus_Bar.new(1)]
d = [T_array_plus_Bar.new(2)]
puts (c + d).length

