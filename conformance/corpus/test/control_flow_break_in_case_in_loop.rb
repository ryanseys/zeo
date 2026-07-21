out = []
i = 0
loop do
  case i
  when 3
    break
  when 7
    break
  end
  out << i
  i = i + 1
  break if i > 50
end
p out
