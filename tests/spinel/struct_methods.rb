# Struct method surface (#2488-2491, #2506, #2534-2539)
S = Struct.new(:a, :b, :c)
p(S[10, 20, 30])                 # 2488 Struct.[]
s = S.new(1, 2, 3)
s[-1] = 99                       # 2489 []= negative index
p(s.c)
p(s.entries)                     # 2490
s.reverse_each { |x| print x }; puts   # 2491
r = s.each_with_index { |x, i| }       # 2506 returns receiver
p(r == s)
p(s.compact)                     # 2538
S2 = Struct.new(:a, keyword_init: true)
p(S2.keyword_init?)              # 2534
p(S.keyword_init?)
p(s.chain([4, 5]).to_a)          # 2535
t = S.new(1, 1, 2)
p(t.slice_before { |x| x == 1 }.to_a)  # 2536
p(t.slice_after { |x| x == 1 }.to_a)
e = s.each_entry                 # 2537 blockless -> Enumerator
p(e.to_a)
require "set"
p(s.to_set)                      # 2539
