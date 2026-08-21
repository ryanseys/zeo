# A symbol's NAME may hold a NUL. Everything downstream of the parser lost it:
# the compiler interned by strdup/strcmp, the emitted name table measured with
# strlen, and the runtime intern compared with strcmp. The node table always
# carried the byte -- nothing was lost before codegen.
#
# A symbol is an integer at run time, so equality, hash lookup and case/when
# never touch a name; only interning, #to_s, ordering and inspect do.
a = :"a\0b"
b = "a\0b".to_sym

p a.to_s.bytesize
p b.to_s.bytesize
p a == b
p a.to_s == "a\0b"
p a.to_s == "a"
p a.to_s.bytes
p a.length
p a.size

# distinct names that share a prefix up to the NUL stay distinct
p :"a\0b" == :"a\0c"
p :"a\0b" == :a
p [:"a\0b", :"a\0c", :a].map { |s| s.to_s.bytesize }

# ordering is byte-exact: strcmp called the first two equal
p(:"a\0b" <=> :"a\0b")
p(:"a\0b" <=> :"a\0c")
p(:a <=> :"a\0b")
p [:"a\0b", :a].sort.map { |s| s.to_s.bytesize }

# hash keys keep them apart
h = { a => 1, :a => 2 }
p h.size
p h[a]
p h[b]
p h[:a]

# and the ordinary symbol surface is untouched
p :abc.to_s
p :abc == :abc
p :abc == :abd
p(:abc <=> :abd)
p({ abc: 1 }[:abc])
p :abc.inspect
p :"a b".inspect
p %i[x y].map(&:to_s)
