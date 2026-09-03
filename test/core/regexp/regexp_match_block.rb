# GAP -- imported from the spinel corpus at c55d9bdb.
# Regexp#match with a block returns the MatchData instead of the block's
# value.
#
# `re.match(str) { |m| ... }` yields the MatchData on a hit and evaluates to
# the block's value. Only the String-receiver form had the arm, so with a
# Regexp receiver the block never ran and the MatchData itself was the value.
p(/a/.match("bab") { |m| m.begin(0) })
p(/(a)/.match("bab") { |m| m[1] })
p(/z/.match("bab") { |m| m.begin(0) })

a = /(a)/
p(a.match("bab") { |m| m[1] })
p(a.match("zzz") { |m| m[1] })

# the String-receiver form and the blockless forms are unchanged
p("bab".match(/a/) { |m| m.begin(0) })
p(/a/.match("bab"))
p(a.match("bab"))
p(/z/.match("bab"))
__END__
1
"a"
nil
"a"
nil
1
#<MatchData "a">
#<MatchData "a" 1:"a">
nil
