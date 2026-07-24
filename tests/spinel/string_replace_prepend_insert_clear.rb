# String#replace / prepend / insert / clear mutate a string held in a
# local variable. The analyzer widens such a local to a mutable_str
# (or, for clear, reassigns it), so these match CRuby instead of being
# no-ops. Both string-literal locals and String.new receivers work.
s = "abc"
s.replace("xyz")
p s

s2 = "world"
s2.prepend("hello ")
p s2

s3 = "hlo"
s3.insert(1, "el")
p s3

s4 = "hello"
s4.clear
p s4.length

# String.new receivers mutate through the same paths.
m = String.new("abc")
m.replace("xyz")
m.prepend("> ")
p m

m2 = String.new("hello")
m2.clear
p m2.length

# insert with a negative index counts from the end.
s5 = "abcd"
s5.insert(-2, "X")
p s5
