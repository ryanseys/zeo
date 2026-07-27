# A poly dispatch drops an arm whose parameter cannot receive the argument.
# It knew about String and object/non-object mismatches, but not about the
# kinds C passes as a struct BY VALUE -- Range, Time, Complex, Rational, Class.
# A struct converts to nothing, and the arm passes its temps raw, so matching
# `s[0...-5]` by name against a `#[](Symbol)` handed an sp_Range to an sp_sym
# slot and the generated C did not compile.
class Tag
  def [](sym) = "sym:#{sym}"
end

class Slice
  def [](rng) = rng.to_a.length
end

xs = [Tag.new, Slice.new]
p xs[0][:abc]
p xs[1][1..4]

# the same name reached from one union, with each arm getting its own kind
class A
  def take(r) = r.to_a.length
  def stamp(t) = t.year > 0
end

class B
  def take(r) = r.to_a.first
  def stamp(t) = t.month > 0
end

both = [A.new, B.new]
both.each { |o| p o.take(1..3) }
both.each { |o| p o.stamp(Time.at(0)) }

# a String slice through an untyped receiver still slices
def slice_it(s)
  s[0...-5]
end
p slice_it("hello.json")
p slice_it("abcdefgh")
