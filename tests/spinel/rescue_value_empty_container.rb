# An empty container's type reads UNKNOWN for want of an element type, which is
# not the same as producing no value. Wherever a rescue-bearing expression asks
# "does this arm carry a value?", the two were conflated: the modifier form
# built the array and threw it away (`x = ([] rescue 0)` assigned nil), and the
# begin/rescue form took the other arm's type alone and put a container pointer
# in an mrb_int slot (#3495, #3496). Both arms of both forms are covered here.
a = ([] rescue 0)
p a
p a.class
p ({} rescue 0)
p (Array.new rescue 0)
p (Hash.new rescue 0)
p ([1] rescue 0)
p (raise("x") rescue 0)

# the value is a real container, not just something that prints like one
b = ([] rescue 0)
b << 1 if b.is_a?(Array)
p b

c = begin
  []
rescue
  0
end
p c

d = begin
  {}
rescue
  0
end
p d

# the empty container on the HANDLER side
e = begin
  raise "e"
rescue
  []
end
p e

f = begin
  Integer("z")
rescue ArgumentError
  {}
end
p f

# both arms empty
g = begin
  []
rescue
  []
end
p g

# a non-empty body still behaves
h = begin
  [1, 2]
rescue
  0
end
p h
