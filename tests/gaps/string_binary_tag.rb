# The BINARY tag a string carries (ASCII-8BIT) had four readers that did not
# agree. pack marks its answer, so its bytes inspect as \xNN and its length
# counts bytes. String#b handed back a plain dup and stayed UTF-8. #encoding
# folded to the constant UTF-8 while discarding the receiver, even though the
# sp_encoding_binary twin already sat next to sp_encoding_utf8. An append that
# outgrew its buffer allocated a fresh header and dropped the tag. And a string
# captured by a block becomes a shared-mutable handle, which inherited the
# frozen bit from its source but not this one.

b = "café".b
p b.length
p "café".length
p b.bytesize
p b.chars
p b.encoding.to_s
p "café".encoding.to_s
p b.frozen?
p b.b.length

# pack was the reader that already agreed
q = [200, 300].pack("C*")
p q
p q.encoding.to_s
p q.length

# the tag survives an append, in place and through a grow
s = "café".b
s << "x"
p s
p s.encoding.to_s

# and through the shared-mutable handle a block capture turns it into
big = "café".b
20.times { big << "0123456789" }
p big.encoding.to_s
p big.length
p big[0, 6]
pk = [200, 300].pack("C*")
3.times { pk << "z" }
p pk
p pk.length

# a plain UTF-8 string is untouched by all of it
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

# The seventh reader: comparison. CRuby'"'"'s rb_str_comparable says equal bytes are
# equal strings only when the encodings are comparable -- the same encoding, or
# both operands ASCII only. So "abc".b == "abc" but "café".b != "café", and
# <=> falls back to the encoding index (ASCII-8BIT sorts first) while hash has
# to separate the two so a Hash keeps them apart.
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

# and as Hash keys, which is where the answer actually shows
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

# sorting mixes them without collapsing them
p ["café".b, "café", "abc"].sort.length
p ["café".b, "café"].uniq.length
p ["abc".b, "abc"].uniq.length
