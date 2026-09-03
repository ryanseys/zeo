# MatchData introspection: begin/end and bytebegin/byteend group positions,
# match/match_length, inspect, and the pattern-matching deconstruct hooks
# (deconstruct for arrays, deconstruct_keys for named captures).
m = "hello world".match(/(\w+)(\s+)(\w+)/)
puts m.begin(1)
puts m.end(3)
puts m.offset(1).inspect

md = "a1b2".match(/(\d)(\w)/)
puts md.bytebegin(1)
puts md.byteend(2)

full = "abc".match(/(a)(b)(c)/)
puts full.inspect
p full.deconstruct
puts full.match(2)
puts full.match_length(2)

named = "2024-01".match(/(?<year>\d+)-(?<mon>\d+)/)
p named.deconstruct_keys(nil)
p named.deconstruct_keys([:year])
p named.deconstruct_keys([:year, :mon, :extra])
p named.named_captures(symbolize_names: true)
__END__
0
11
[0, 5]
1
3
#<MatchData "abc" 1:"a" 2:"b" 3:"c">
["a", "b", "c"]
b
1
{year: "2024", mon: "01"}
{year: "2024"}
{}
{year: "2024", mon: "01"}
