# An embedded NUL survives the regexp surface. The engine itself was already
# byte-exact; what truncated was the code AROUND it -- capture strings built
# with sp_str_alloc_raw and no length set, the scan bound taken with strlen,
# a replacement sized with strlen, and split's "is this tail empty?" test
# reading s[0] rather than the byte length.
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

# The display surface renders the source rather than ending at the NUL.
# (What it renders it AS is String#inspect's business: this engine spells a
# control byte \u0000 where CRuby, reading the name as US-ASCII, spells it
# \x00 -- the same difference "x".b.inspect already has, and not a truncation.)
r("source bytes")  { Regexp.new("a\0b").source.bytes }
r("to_s bytes")    { Regexp.new("a\0b").to_s.bytes }
r("inspect bytes") { Regexp.new("a\0b").inspect.bytes }
r("escape")        { Regexp.escape("a\0b").bytes }
r("union 1")       { Regexp.union("a\0b").source.bytes }
r("union 2")       { Regexp.union("a\0b", "z").source.bytes }
r("union match")   { ("a\0b" =~ Regexp.union("a\0b", "z")) }
r("to_s slash")    { Regexp.new("a/b").to_s }

# String#split on a subject whose pieces START with NUL: the trailing-empty
# trim read the first byte, so every such piece was dropped.
big = "\0" * 5 + "x" + "\0" * 5
r("split pieces")  { big.split("x").map { |q| q.bytesize } }
r("split mid")     { "\0x\0".split("x").map { |q| q.bytesize } }
r("split plain")   { "ax b".split("x").map { |q| q.bytesize } }

# and the MatchData faces
m = s.match(/a(.)b(.)c/m)
r("to_a")          { m.to_a }
r("captures")      { m.captures }
r("values_at")     { m.values_at(1, 2) }
r("named")         { s.match(/a(?<p>.)b/m).named_captures }
r("begin/end")     { [m.begin(1), m.end(1)] }
