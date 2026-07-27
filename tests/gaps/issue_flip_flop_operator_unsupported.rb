# The flip-flop operator (`expr1..expr2` used as a boolean in a conditional)
# isn't lowered at all -- zeo rejects it at compile time instead of tracking
# the flip-flop's on/off state across loop iterations.
results = []
(1..10).each do |i|
  results << i if (i == 3)..(i == 6)
end
p results
