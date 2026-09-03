# MatchData begin/end, bytebegin/byteend, match, inspect, deconstruct, and
# deconstruct_keys with CRuby's key rules.

m = "hello world".match(/(\w+)(\s+)(\w+)/)
p [m.begin(1), m.end(1), m.begin(3)]
md = "a1b2".match(/(\d)(\w)/); p [md.bytebegin(1), md.byteend(1)]
m2 = "abc".match(/(a)(b)(c)/)
p m2.inspect
p m2.deconstruct
p m2.match(2)
n = "abc".match(/(?<x>b)(?<y>c)/)
p [n.deconstruct_keys([:x]), n.deconstruct_keys([:y, :x]), n.deconstruct_keys(nil), n.deconstruct_keys([:x, :y, :z])]
p n.named_captures(symbolize_names: true)
__END__
[0, 5, 6]
[1, 2]
"#<MatchData \"abc\" 1:\"a\" 2:\"b\" 3:\"c\">"
["a", "b", "c"]
"b"
[{x: "b"}, {y: "c", x: "b"}, {x: "b", y: "c"}, {}]
{x: "b", y: "c"}
