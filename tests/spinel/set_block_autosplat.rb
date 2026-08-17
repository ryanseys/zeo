require "set"

s = Set.new([[1, 2]])
p s.map { |r, c| [r, c] }
s.each { |r, c| p [r, c] }
p s.map { |r, c| r }
p s.select { |r, c| r == 1 }.to_a
p s.sum { |r, c| r }
p s.sort_by { |r, c| r }

t = Set.new([1, 2])
p t.map { |x| x + 1 }
t.each { |x| p x }

h = { a: 1 }
h.each { |k, v| p [k, v] }
p [[3, 4]].map { |r, c| [r, c] }

class Y
  def initialize(d); @d = d; end
  def each
    i = 0
    while i < @d.size
      yield @d[i]
      i += 1
    end
    self
  end
end
Y.new([[5, 6]]).each { |r, c| p [r, c] }
Y.new([7]).each { |r, c| p [r, c] }
Y.new([[8, 9]]).each { |x| p x }
