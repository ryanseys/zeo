Float::NAN.step(2.0, 0.5) { |x| p x }; puts "a"
1.0.step(Float::NAN, 0.5) { |x| p x }; puts "b"
n = 0
Float::INFINITY.step(2.0, 0.5) { |x| n += 1; break if n > 3 }
p n
1.0.step(3.0, 0.5) { |x| p x }
