# Integer#step with no limit iterates until the block breaks.
r = 1.step { |i| break i if i > 3 }
p r
n = 0
5.step { |i| n += i; break if i >= 8 }
p n
