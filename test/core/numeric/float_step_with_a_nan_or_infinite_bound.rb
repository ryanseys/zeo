# NaN as the start or the limit yields nothing; an infinite start yields
# forever, so the block breaks.
Float::NAN.step(2.0, 0.5) { |x| p x }; puts "a"
1.0.step(Float::NAN, 0.5) { |x| p x }; puts "b"
n = 0
Float::INFINITY.step(2.0, 0.5) { |x| n += 1; break if n > 3 }
p n
1.0.step(3.0, 0.5) { |x| p x }
__END__
a
b
0
1.0
1.5
2.0
2.5
3.0
