# Struct#to_h with a block yields each (member_symbol, value) pair and
# builds a hash from the [k, v] arrays the block returns. Values are
# asserted via key lookup rather than Hash#inspect so the test does not
# depend on the `=>` inspect spacing, which differs across Ruby versions.
Person = Struct.new(:name, :age)
p = Person.new("Alice", 30)
h = p.to_h { |name, val| [name.to_s, val] }
puts h["name"]
puts h["age"]
puts h.size

Point = Struct.new(:x, :y)
pt = Point.new(3, 4)
# transform the value too
h2 = pt.to_h { |k, v| [k.to_s, v * 2] }
puts h2["x"]
puts h2["y"]

# both elements stringified (block returns a str_array)
h3 = pt.to_h { |k, v| [k.to_s, v.to_s] }
puts h3["x"]
puts h3["y"]

# heterogeneous members (string, int, bool)
Rec = Struct.new(:label, :count, :on)
r = Rec.new("widget", 7, true)
h4 = r.to_h { |k, v| [k.to_s, v] }
puts h4["label"]
puts h4["count"]
puts h4["on"]
