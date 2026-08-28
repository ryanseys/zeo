b = "café".b
p b.length
p "café".length
p b.bytesize
p b.chars
p b.encoding.to_s
p "café".encoding.to_s
p b.frozen?
p b.b.length

q = [200, 300].pack("C*")
p q
p q.encoding.to_s
p q.length

s = "café".b
s << "x"
p s
p s.encoding.to_s

big = "café".b
20.times { big << "0123456789" }
p big.encoding.to_s
p big.length
p big[0, 6]
pk = [200, 300].pack("C*")
3.times { pk << "z" }
p pk
p pk.length

u = String.new("café")
u << "x"
p u
p u.encoding.to_s
p u.length
p("caf" + "é")
v = String.new("café")
3.times { v << "y" }
p v.length
p v.encoding.to_s

p("abc".b == "abc")
p("abc".b.eql?("abc"))
p("abc".b <=> "abc")
p("abc".b.hash == "abc".hash)
p("café".b == "café")
p("café".b.eql?("café"))
p("café".b <=> "café")
p("café" <=> "café".b)
p("café".b.hash == "café".hash)
p("café".b == "café".b)
p("café" == "café")
p("abc".b == "abd")
p([200, 300].pack("C*") == [200, 300].pack("C*"))

h = {}
h["café"] = 1
h["café".b] = 2
p h.size
p h["café"]
g = {}
g["abc"] = 1
g["abc".b] = 2
p g.size
p g["abc"]

p ["café".b, "café", "abc"].sort.length
p ["café".b, "café"].uniq.length
p ["abc".b, "abc"].uniq.length
