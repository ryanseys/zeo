b = "x"
p((1..5).cover?(b))
def g; "x"; end
p((1..5).cover?(g))
b = [1]; p((1..5).cover?(b))
b = { a: 1 }; p((1..5).include?(b))
b = :sym; p((1..5).member?(b))
b = nil;  p((1..5) === b)
b = 3;    p((1..5).cover?(b))
b = 2.5;  p((1..5).cover?(b))

# a Range operand: cover? is containment, === and include? are not
p((1..5).cover?(1...6))
p((1..5).cover?(2..9))
p((1..5).cover?(1.0..5.0))
p((1..5).cover?(1.0...5.5))
p((1...5).cover?(1..4))
p((1..5) === (1.0..5.0))
p((1..5).include?(1..2))
r = ((1..5).eql?(1.0..5.0) rescue $!.class); p r
fr = (1.0..5.0); r = ((1..5).eql?(fr) rescue $!.class); p r
