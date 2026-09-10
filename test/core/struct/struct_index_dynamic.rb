# The out-of-range Struct#[] message says "too large" for a negative
# offset; ruby says "too small".
#
# Struct#[] with an index that is not a literal: the member chain was emitted
# as bare statements, so the whole read was a void expression and the C would
# not compile. A negative offset counts from the end, and a miss raises.
S = Struct.new(:a, :b)
s = S.new(1, 2)
i = 1
p s[i]
k = -1
p s[k]
m = -9
p((s[m] rescue $!.message))
n = 9
p((s[n] rescue $!.message))
p s[0]
p s[:a]
j = :b
p s[j]
p((s[:zz] rescue $!.message))
p s["a"]
__END__
2
2
"offset -9 too small for struct(size:2)"
"offset 9 too large for struct(size:2)"
1
1
2
"no member 'zz' in struct"
1
