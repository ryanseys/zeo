# A constant assigned nil still needs a slot, and String#scan reaches a value
# read out of a `{}`-then-filled Hash.
EPSILON = nil
p EPSILON
p EPSILON.nil?
p EPSILON.inspect
OTHER = 5
p OTHER

h = {}
h["A1"] = "a1b2"
p h["A1"].scan(/\d+/)
p h["A1"].scan(/(\w)(\d)/)
p h["A1"].scan("b")
p h["A1"].split("b")
p h["A1"].length

# a statically-typed String still takes the direct path
s = "a1b2"
p s.scan(/\d+/)
