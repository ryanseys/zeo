# A method whose body forwards to the SAME name on a union-typed receiver
# resolved the call to itself -- the only class defining that name -- so the
# return type union was the method's own unfinished answer and the whole thing
# came out void: `zero?` answered nil for every receiver, and `<=>` returned
# nil and took Comparable's operators down with it (#3488, #3490).
class Q
  def initialize(v)
    @value = v
  end

  def zero?
    @value.zero?
  end

  def positive?
    @value.positive?
  end
end

p Q.new(0).zero?
p Q.new(Rational(0)).zero?
p Q.new(1).zero?
p Q.new(1).positive?
p Q.new(Rational(-1)).positive?

class M
  include Comparable
  attr_reader :v

  def initialize(v)
    @v = v
  end

  def <=>(other)
    @v <=> other.v
  end

  # never called: defining it is enough to widen the class's ivar, which is
  # what used to make <=> void
  def widen(other)
    other = M.new(other) unless other.is_a?(M)
    other.v
  end
end

p(M.new(1) <=> M.new(2))
p(M.new(2) <=> M.new(1))
p(M.new(1) <=> M.new(1))
p(M.new(1) < M.new(2))
p(M.new(2) > M.new(1))
p(M.new(1) == M.new(1))
p [M.new(3), M.new(1), M.new(2)].sort.map(&:v)
p M.new(2).between?(M.new(1), M.new(3))
p M.new(5).clamp(M.new(1), M.new(3)).v
