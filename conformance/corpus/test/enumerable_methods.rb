# Enumerable/Enumerator surface (#2479, #2480, #2486, #2504, #2505, #2516)
class C
  include Enumerable
  def each; yield 3; yield 1; yield 2; end
end
c = C.new
p(c.grep(1..2))                       # 2479 grep with Range
p(c.grep_v(2))                        # 2479 grep_v
p(c.each_entry.class)                 # 2480 blockless each_entry -> Enumerator
e = [10, 20].each
e.each_with_index { |x, i| p [x, i] } # 2486 each_with_index on an Enumerator
p(c.slice_before { |x| x == 1 }.to_a) # 2505 slice_before on a user Enumerable
p([1.5, 2.5].sum)                     # 2516 Float sum with default init
begin; [1, 2].sum(""); rescue => ex; p ex.class; end   # 2504 numeric sum, String init
begin; c.sum("X"); rescue => ex; p ex.class; end
