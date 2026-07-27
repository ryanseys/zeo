list, kept = ["--", "x"], []
while (i = list.shift)
  case i
  when "--" then break kept.concat(list)
  end
end
p kept
n = 0
while true
  break (n += 5)
end
p n
