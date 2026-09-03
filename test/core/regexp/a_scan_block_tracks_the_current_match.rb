# `String#scan` with a block sets `$~` to the CURRENT match on each iteration,
# exactly as `sub`/`gsub`'s block forms do. The runtime's `scan` row collected
# every match first and only then yielded, so `$~` sat at the LAST match for
# every iteration; the rustc backend hid that behind a typed fast path, which
# only fires when the receiver is statically a String -- so a receiver that is
# not, or any call through the dynamic path, read the wrong match.
#
# The `$~` a block reads belongs to the enclosing METHOD's svar scope, and
# `$~` is frame-local: a method that matches and returns leaves the caller's
# `$1` alone. Both rules are in play here at once.

s = "1a2b3c"

# The receiver is statically a String (the typed path).
seen = []
s.scan(/\d/) { seen << $~[0] }
p seen

# The same receiver reached dynamically -- an array element the compiler
# cannot type.
box = [s]
seen = []
box[0].scan(/\d/) { seen << $~[0] }
p seen

# A capture group: `$1` and the yielded value agree.
pairs = []
s.scan(/([a-z])/) { |g| pairs << [g, $1, $~[0]] }
p pairs

# The whole MatchData is live, not just the groups.
around = []
s.scan(/b/) { around << [$~.pre_match, $~.post_match] }
p around

# A match inside the block is what the NEXT read sees, until the next yield
# replaces it.
inner = []
s.scan(/\d/) do
  "zz" =~ /(z)/
  inner << $1
end
p inner

# `$~` is frame-local: the scan above ran at the top level, and a method that
# scans leaves the caller's match alone.
def counts(str)
  n = 0
  str.scan(/\d/) { n += 1 }
  n
end
"abc" =~ /(b)/
p counts(s)
p $1

# A block that breaks stops the walk, and the match it broke on stands.
"abc" =~ /(b)/
def first_digit(str)
  str.scan(/(\d)/) { |g| break g.first }
end
p first_digit(s)
p $1

# The block form answers the RECEIVER, never the array of matches.
p s.scan(/\d/) { }
__END__
["1", "2", "3"]
["1", "2", "3"]
[[["a"], "a", "a"], [["b"], "b", "b"], [["c"], "c", "c"]]
[["1a2", "3c"]]
["z", "z", "z"]
3
"b"
"1"
"b"
"1a2b3c"
