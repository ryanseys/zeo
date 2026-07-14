h = {}
h[:a] = 1
h[:b] = 2
h[:c] = 3
h[:a] = 99
puts h
puts h.length

pairs = {}
pairs[[1, 2]] = "one-two"
pairs[[3, 4]] = "three-four"
puts pairs[[1, 2]]
puts pairs[[3, 4]]

x = 5
puts x.is_a?(Integer)
puts x.is_a?(String)
puts x.is_a?(Object)
puts x.kind_of?(Integer)

arr = [1, 2, 3]
puts arr.is_a?(Array)
puts arr.is_a?(Hash)

r = 1..5
puts r.is_a?(Range)

sym = :foo
puts sym.is_a?(Symbol)

puts nil.is_a?(NilClass)
puts true.is_a?(TrueClass)
puts false.is_a?(FalseClass)

class Checker
  def check(v)
    v.is_a?(Integer)
  end
end
c = Checker.new
puts c.check(5)
puts c.check("hi")
