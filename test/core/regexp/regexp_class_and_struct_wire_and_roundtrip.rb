# `/` (regexp source + option byte, `I`-wrapped for encoding), `c`/`m`
# (class/module reference), and `S` (Struct: class symbol, member count,
# member/value pairs).

def wire(x) = Marshal.dump(x).bytes.join(",")
def rt(x) = Marshal.load(Marshal.dump(x))
puts wire(/ab.c/im)
r = rt(/a\d+b/i)
puts r.source
puts r.options
puts wire(String)
p rt(String)
p rt(Comparable)
S = Struct.new(:a, :b)
puts wire(S.new(1, 2))
st = rt(S.new(10, "x"))
p [st.a, st.b]
__END__
4,8,73,47,9,97,98,46,99,5,6,58,6,69,70
a\d+b
1
4,8,99,11,83,116,114,105,110,103
String
Comparable
4,8,83,58,6,83,7,58,6,97,105,6,58,6,98,105,7
[10, "x"]
