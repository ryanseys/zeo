# Index assignment and an append inside a method both reach the caller's array.
def setit(arr, i)
  arr[i] = 99
end
a = Array.new(4) { |i| i }
setit(a, 1)
p a

def bump(arr)
  arr.each_index { |k| arr[k] += 1 }
end
b = Array.new(3) { |i| i * 10 }
bump(b)
p b

# element type still narrows correctly for a plain-literal-element block
def push_it(arr)
  arr << 5
end
c = Array.new(2) { [] }
push_it(c)
p c
__END__
[0, 99, 2, 3]
[1, 11, 21]
[[], [], 5]
