require "ostruct"

o = OpenStruct.new(a: 1, b: "hi")
p o.a
p o.b
o.c = [1, 2]
p o.c
p o[:a]
o[:d] = 9
p o[:d]
p o.to_h
p o.respond_to?(:a)
p o.respond_to?(:z)
p o.class
p o
p o.is_a?(OpenStruct)
p o.is_a?(Struct)
p o.instance_of?(OpenStruct)

o2 = OpenStruct.new(a: 1, b: "hi", c: [1, 2], d: 9)
p o == o2
o2.d = 10
p o == o2
p o.nothere

empty = OpenStruct.new
p empty.to_h
empty.set = "later"
p empty.set

arr = [OpenStruct.new(x: 1), "s", 42]
p arr

def show(v)
  p v
end
show(OpenStruct.new(n: 7))
