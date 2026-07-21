# MatchData reflection/pattern methods (#2499, #2500, #2501, #2503, #2530, #2532)
md = "abc".match(/(a)(b)(c)/)
p md.inspect
p md.regexp
p md.match(1)
p md.match_length(2)
p md.deconstruct
p md[1..2]
p md[1...3]
p md[0..-1]
md2 = "abc".match(/(?<x>a)(?<y>b)/)
p md2.named_captures(symbolize_names: true)
p md2.deconstruct_keys(nil)
p md2.named_captures
p md2.inspect
