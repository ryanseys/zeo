# The flip-flop operator: `cond1..cond2` used AS a condition is a two-state
# latch, not a Range. Two dots retest the right operand in the evaluation that
# turned the latch on; three dots wait for the next one. An omitted side is nil,
# so it never fires. Each syntactic occurrence owns its latch, and that latch
# survives between separate runs of the loop it sits in.
two_dot = []
(1..8).each { |i| two_dot << i if (i == 3)..(i == 6) }
p two_dot

three_dot = []
(1..8).each { |i| three_dot << i if (i == 3)...(i == 6) }
p three_dot

# Same operand on both sides: two dots span one iteration, three dots never
# close, because the right operand is only ever tested after the latch is on.
one_shot = []
(1..8).each { |i| one_shot << i if (i == 3)..(i == 3) }
p one_shot

never_closes = []
(1..8).each { |i| never_closes << i if (i == 3)...(i == 3) }
p never_closes

# Beginless never turns on; endless never turns off.
beginless = []
(1..5).each { |i| beginless << i if ..(i == 3) }
p beginless

endless = []
(1..5).each { |i| endless << i if (i == 2).. }
p endless

# Two occurrences in one loop body keep separate latches.
pairs = []
(1..6).each do |i|
  a = false
  b = false
  a = true if (i == 2)..(i == 3)
  b = true if (i == 4)..(i == 5)
  pairs << [i, a, b]
end
p pairs

# One occurrence keeps its latch across separate runs of its loop: the second
# pass starts with the latch still off, having closed on 3 in the first.
across = []
2.times { (1..4).each { |i| across << i if (i == 2)..(i == 3) } }
p across
