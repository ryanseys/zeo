r = loop { break 42 }
puts r.inspect
puts r.class

r2 = loop {
  x = 0
  loop { x += 1; break if x > 3 }
  break x
}
puts r2.inspect

i = 0
loop { i += 1; break if i >= 5 }
puts i
