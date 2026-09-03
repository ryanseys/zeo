# A user subclass of `Array` (D3) is the native `ValueSubclass` wrapping an
# array payload: inherited methods (`push`/`<<`/`size`/`each`/`map`) run
# against it, self-returning mutators re-wrap to the subclass while a NEW
# collection (`map`) is a plain `Array`, and a custom method sees the elements.

class Stack < Array
  def peek; last; end
end
s = Stack.new
s.push(1)
s << 2
p s
puts s.size
puts s.peek
puts s.class
puts s.push(3).class
puts s.map { |x| x * 2 }.inspect
puts s.map { |x| x }.class
puts s.is_a?(Array)
puts Stack.new([9, 8]).size
__END__
[1, 2]
2
2
Stack
Stack
[2, 4, 6]
Array
true
2
