# A user class (and a core class) reports frozen? == false. Hash#delete
# calls its block when the key is absent. $/ defaults to "\n".

class C001; end
p C001.frozen?
p Integer.frozen?
d = { a: 1, b: 2 }
p d.delete(:b)
p d.delete(:z) { |k| "gone #{k}" }
p $/
__END__
false
false
2
"gone z"
"\n"
