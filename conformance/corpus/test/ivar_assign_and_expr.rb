# `@x = expr and rest` parses as `(@x = expr) and rest`; @x must be
# typed from expr (poly), not the boolean `and` result.
class C
  def initialize(o = {})
    @debug = o[:debug] and o[:debug] == true
  end
  def debug; @debug; end
end
p C.new(debug: true).debug
p C.new.debug
# `@x = @x + 1` as a method's value returns the (boxed) slot value.
class A
  def initialize(attrs = {})
    @id = attrs[:id] || 0
  end
  def bump = (@id = @id + 1)
  def id = @id
end
a = A.new(id: 10)
a.bump
a.bump
puts a.id
