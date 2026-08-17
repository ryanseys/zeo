$calls = 0

def src
  $calls += 1
  [1, 2, 3]
end

a = [0]
a.concat(src)
p a
p $calls

b = []
b.concat(src, src)
p b
p $calls

# a string's bytes: the source is a fresh array each time it is evaluated
out = []
out.concat("abc".bytes)
out.concat("de".bytes)
p out

# mixed kinds still coerce
poly = [:x]
poly.concat([1, 2])
p poly
ints = [9]
ints.concat([1, 2].map { |v| v })
p ints
strs = ["a"]
strs.concat(%w[b c])
p strs
