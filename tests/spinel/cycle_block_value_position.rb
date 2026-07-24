# Array#cycle(n) { } with its return value captured: the statement loop
# runs against a hoisted receiver and the expression evaluates to nil
# (a valued break still routes through the break wrapper).
x = [1, 2, 3].cycle(2) { |y| }
p x
seen = []
r = [4, 5].cycle(3) { |y| seen << y }
p r
p seen
b = [1, 2].cycle { |y| break y * 100 if y == 2 }
p b
