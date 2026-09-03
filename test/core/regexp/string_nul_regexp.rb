s = "a\0b\0c"
def r(l); v = yield; puts "#{l}: #{v.inspect}"; end

r("scan .")        { s.scan(/./m) }
r("scan (.)")      { s.scan(/(.)/m) }
r("scan NUL")      { s.scan(/\0/) }
r("$~ capture")    { s =~ /a(.)b/m; $~[1] }
r("$1")            { s =~ /a(.)b/m; $1 }
r("post_match")    { s.match(/b/).post_match }
r("pre_match")     { s.match(/b/).pre_match }
r("gsub block")    { s.gsub(/./m) { |m| m.bytes.inspect } }
r("split keep")    { s.split(/(\0)/) }
r("slice re")      { s[/a(.)b/m, 1] }
r("sub rep NUL")   { "ab".sub(/b/, "x\0y") }
r("gsub rep NUL")  { "ab".gsub(/./, "\0") }
r("gsub blk NUL")  { "ab".gsub(/./) { "\0" } }
r("Regexp.new")    { s =~ Regexp.new("a\0b") }
r("source")        { Regexp.new("a\0b").source.bytes }
r("match at pos")  { s.match(/\0/, 2)[0].bytes }

r("source bytes")  { Regexp.new("a\0b").source.bytes }
r("to_s bytes")    { Regexp.new("a\0b").to_s.bytes }
r("inspect bytes") { Regexp.new("a\0b").inspect.bytes }
r("escape")        { Regexp.escape("a\0b").bytes }
r("union 1")       { Regexp.union("a\0b").source.bytes }
r("union 2")       { Regexp.union("a\0b", "z").source.bytes }
r("union match")   { ("a\0b" =~ Regexp.union("a\0b", "z")) }
r("to_s slash")    { Regexp.new("a/b").to_s }

big = "\0" * 5 + "x" + "\0" * 5
r("split pieces")  { big.split("x").map { |q| q.bytesize } }
r("split mid")     { "\0x\0".split("x").map { |q| q.bytesize } }
r("split plain")   { "ax b".split("x").map { |q| q.bytesize } }

m = s.match(/a(.)b(.)c/m)
r("to_a")          { m.to_a }
r("captures")      { m.captures }
r("values_at")     { m.values_at(1, 2) }
r("named")         { s.match(/a(?<p>.)b/m).named_captures }
r("begin/end")     { [m.begin(1), m.end(1)] }
__END__
scan .: ["a", "\u0000", "b", "\u0000", "c"]
scan (.): [["a"], ["\u0000"], ["b"], ["\u0000"], ["c"]]
scan NUL: ["\u0000", "\u0000"]
$~ capture: "\u0000"
$1: "\u0000"
post_match: "\u0000c"
pre_match: "a\u0000"
gsub block: "[97][0][98][0][99]"
split keep: ["a", "\u0000", "b", "\u0000", "c"]
slice re: "\u0000"
sub rep NUL: "ax\u0000y"
gsub rep NUL: "\u0000\u0000"
gsub blk NUL: "\u0000\u0000"
Regexp.new: 0
source: [97, 0, 98]
match at pos: [0]
source bytes: [97, 0, 98]
to_s bytes: [40, 63, 45, 109, 105, 120, 58, 97, 92, 120, 48, 48, 98, 41]
inspect bytes: [47, 97, 92, 120, 48, 48, 98, 47]
escape: [97, 0, 98]
union 1: [97, 0, 98]
union 2: [97, 0, 98, 124, 122]
union match: 0
to_s slash: "(?-mix:a\\/b)"
split pieces: [5, 5]
split mid: [1, 1]
split plain: [1, 2]
to_a: ["a\u0000b\u0000c", "\u0000", "\u0000"]
captures: ["\u0000", "\u0000"]
values_at: ["\u0000", "\u0000"]
named: {"p" => "\u0000"}
begin/end: [1, 2]
