# A group name written twice answers the LAST group of that name that took
# part in the match -- by `[]`, `begin`, `offset`, `named_captures`,
# `deconstruct_keys` and a `\k<n>` replacement alike -- and the last group of
# that name when none did. `names` lists it once.
m = "ab".match(/(?<n>a)|(?<n>b)/)
p m[:n], m.named_captures, m.names, m.begin(:n), m.values_at(:n)
m = "b".match(/(?<n>a)|(?<n>b)/)
p m[:n], m.named_captures, m.begin(:n)
m = "ab".match(/(?<n>a)(?<n>b)/)
p m, m[:n], m.named_captures(symbolize_names: true), m.offset(:n), m.byteoffset(:n), m.deconstruct_keys(nil)
m = "a".match(/(?<n>a)|(?<n>b)(?<x>c)?/)
p m, m.named_captures, m.deconstruct_keys([:n, :x]), m.end(:n)
p "xab".sub(/(?<n>a)|(?<n>b)/, "<\\k<n>>"), "ab".gsub(/(?<n>a)|(?<n>b)/, "[\\k<n>]")
p "ab".scan(/(?<n>a)|(?<n>b)/)
"b" =~ /(?<n>a)|(?<n>b)/
p $~[:n]
require "strscan"
s = StringScanner.new("b")
s.scan(/(?<n>a)|(?<n>b)/)
p s[:n], s.named_captures
__END__
"a"
{"n" => "a"}
["n"]
0
["a"]
"b"
{"n" => "b"}
0
#<MatchData "ab" n:"a" n:"b">
"b"
{n: "b"}
[1, 2]
[1, 2]
{n: "b"}
#<MatchData "a" n:"a" n:nil x:nil>
{"n" => "a", "x" => nil}
{n: "a", x: nil}
1
"x<a>b"
"[a][b]"
[["a", nil], [nil, "b"]]
"b"
"b"
{"n" => "b"}
