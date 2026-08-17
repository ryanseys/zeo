# A proc handed to a method is called from there as well as from the scope
# that made it. The two sites pass different types, so the params cannot be
# pinned to the visible one.
sink = proc { |x| x.size }

def feed(s)
  rows = [[1], [2], [3]]
  s.call(rows)
end

p feed(sink)
p sink.call([1, 2])
p sink.call("abcd")

double = proc { |n| n * 2 }

def apply(f, v)
  f.call(v)
end

p apply(double, 21)
p double.call(3)
