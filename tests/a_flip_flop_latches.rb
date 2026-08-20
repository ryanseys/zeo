# A flip-flop is a LATCH, not a Range: true from the evaluation whose left
# operand is truthy through the one whose right operand is. Two dots retest
# the right operand in the very evaluation that turned it on; three wait.
(1..10).each do |i|
  print i if (i == 3)..(i == 6)
end
puts
(1..10).each do |i|
  print i if (i == 3)...(i == 3)
end
puts
(1..10).each do |i|
  print i if (i == 3)..(i == 3)
end
puts
lines = ["a", "START", "x", "y", "END", "b"]
lines.each do |l|
  puts l if l == "START"..l == "END"
end
2.times do
  (1..5).each do |i|
    print i if (i == 2)..(i == 4)
  end
  puts
end
