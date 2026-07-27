f = ->(s) { s = s[1..]; s }
p f.call("abc")
f2 = ->(a, b) { a = a + 1; b = b * 2; [a, b] }
p f2.call(1, 2)
pr = proc { |s| s = s.to_s; s }
p pr.call(5)
