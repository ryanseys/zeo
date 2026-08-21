# An empty `[]` / `{}` assigned to a class variable carries no element or key
# type, so the slot was declared sp_int (the codegen fallback) while the write
# emitted a real container -- the C build failed on the store. The variant now
# comes from the class variable's own usage, the way a global's and a
# constant's already did (#4054).

# a bare `{}` with no index writes at all still needs a declarable slot
class Empty
  @@h = {}
  def self.h = @@h
end
puts Empty.h.size

# String keys
class StrKeyed
  @@h = {}
  def self.add(k, v) = @@h[k] = v
  def self.get(k) = @@h[k]
  def self.size = @@h.size
end
StrKeyed.add("a", 1)
StrKeyed.add("b", 2)
puts StrKeyed.get("a")
puts StrKeyed.get("b")
puts StrKeyed.size

# Symbol keys take the symbol-keyed variant, not the String default
class SymKeyed
  @@h = {}
  def self.add(k) = @@h[k] = true
  def self.on?(k) = @@h[k] ? true : false
end
SymKeyed.add(:x)
puts SymKeyed.on?(:x)
puts SymKeyed.on?(:y)

# an empty `[]` holds whatever is pushed later
class Bag
  @@a = []
  def self.add(x) = @@a.push(x)
  def self.all = @@a
end
Bag.add("s")
Bag.add(2)
p Bag.all

# a bare `[]` never pushed to
class EmptyBag
  @@a = []
  def self.all = @@a
end
p EmptyBag.all

# `Hash.new` with no arguments is the same empty producer
class Made
  @@h = Hash.new
  def self.add(k, v) = @@h[k] = v
  def self.get(k) = @@h[k]
end
Made.add("k", "v")
puts Made.get("k")

# a non-empty literal keeps working
class Seeded
  @@h = {"a" => 1}
  @@a = [1, 2]
  def self.h = @@h
  def self.a = @@a
end
puts Seeded.h["a"]
p Seeded.a
