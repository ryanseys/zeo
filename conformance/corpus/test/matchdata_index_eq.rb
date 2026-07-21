# MatchData#[] with a negative group index or a (start, length) pair, and
# MatchData#== / #eql? (#2507, #2529, #2531).
md = "abc".match(/(a)(b)(c)/)
p md[-1]
p md[-2]
p md[1, 2]
p md[0, 2]
p md[2, 5]
p md.eql?(md)
p(md == "abc".match(/(a)(b)(c)/))
p(md == "abd".match(/(a)(b)(d)/))
p(md == nil)
md2 = "hello".match(/(?<x>l+)/)
p md2[-1]
