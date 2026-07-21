# String#chars/#bytes/#codepoints/#lines with a block yield each element
# and return the receiver (self), not the element array. The no-block
# form still returns the array.
s = "abc"

r1 = s.chars { |c| print c }
puts ""
puts r1

sum = 0
r2 = s.bytes { |b| sum = sum + b }
puts sum
puts r2

cps = 0
r3 = "ab".codepoints { |cp| cps = cps + cp }
puts cps
puts r3

n = 0
r4 = "x\ny\n".lines { |l| n = n + 1 }
puts n
puts r4

# No-block forms still return arrays.
p "abc".chars
p "abc".bytes
