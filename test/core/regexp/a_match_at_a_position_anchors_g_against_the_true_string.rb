s = "aaa bbb ccc"

# \G anchors at the search start position, not at the string start.
p s.match(/\G\w+/, 0)[0]
p s.match(/\G\w+/, 4)[0]
p s.match(/\Gbbb/, 4)[0]
p s.match(/\Gccc/, 4)
p s.match(/\G\s/, 3)[0]

# The scan loop StringScanner is built from: match at pos, advance by end(0).
pos = 0
tokens = []
while (m = s.match(/\G\s*(\w+)/, pos))
  tokens << m[1]
  pos = m.end(0)
end
p tokens
p pos

# MatchData offsets are absolute, never slice-relative.
m = s.match(/\G(b+)/, 4)
p m.begin(0)
p m.begin(1)
p m.pre_match
p m.post_match

# Regexp#match takes the same optional position.
re = /\G(?<word>\w+)/
m = re.match(s, 8)
p m[:word]
p m.begin(0)
p re.match(s, 3)
p re.match(s, -3)[0]
p re.match(s, 11)
p re.match(s, 12)
p re.match(s, -99)

# A positioned Regexp#match still fills $~.
/\G(b+)/.match(s, 4)
p $~[1]

# ^ beside \G takes a different engine route than a plain \G pattern.
t = "one\ntwo\nthree"
p t.match(/\G\w+$/, 4)[0]
p t.match(/^\w+$/, 5)[0]

# Lookaround beside \G is the backtracking route.
p s.match(/\G(?=a)a+/, 0)[0]
p s.match(/\G(?=b)/, 4).begin(0)
p s.match(/\G(?!z)\w+/, 8)[0]

# Lookbehind sees the true string to the LEFT of the position.
p s.match(/(?<= )\w+/, 3)[0]
p s.match(/\G(?<=a )bbb/, 4)[0]
p s.match(/\G(?<=z )bbb/, 4)

# The block form runs on a hit and the call answers the block's value.
p(/\G\w+/.match(s, 4) { |md| md[0].upcase })
p(/\Gzzz/.match(s, 4) { |md| :never })

# match? at a position, same anchoring, no MatchData.
p(/\Gbbb/.match?(s, 4))
p(/\Gbbb/.match?(s, 3))
__END__
"aaa"
"bbb"
"bbb"
nil
" "
["aaa", "bbb", "ccc"]
11
4
4
"aaa "
" ccc"
"ccc"
8
nil
"ccc"
nil
nil
nil
"bbb"
"two"
"three"
"aaa"
4
"ccc"
"bbb"
"bbb"
nil
"BBB"
nil
true
false
