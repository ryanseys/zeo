# User subclasses of Array/String/Hash are the native ValueSubclass (D3): they
# wrap a payload of the root kind, inherit its methods through the payload
# bridge, re-wrap self-returning mutators, and compare/hash by their payload.

class Stack < Array
  def peek
    last
  end
end

s = Stack.new
s.push(1)
s << 2
s.push(3)
p s
puts s.size
puts s.peek
puts s.pop
puts s.class
puts s.push(4).class          # self-return re-wraps to Stack
puts s.map { |x| x * 10 }.inspect
puts s.map { |x| x }.class    # a NEW array is a plain Array
puts s.is_a?(Array)
puts s.is_a?(Enumerable)
puts Stack.new([5, 6, 7]).sum

class Tag < String
  def shout
    upcase + "!"
  end
end

t = Tag.new("hi")
puts t.shout
puts t.class
puts(t == "hi")
puts("hi" == t)                # symmetric ==
puts(Tag.new("k").hash == "k".hash)
lookup = { "k" => 42 }
puts lookup[Tag.new("k")]      # payload Hash-key equality

class Counter < Hash
  def initialize
    super(0)
  end
end

c = Counter.new
c[:a] += 1
c[:a] += 1
c[:b] += 1
puts c[:a]
puts c[:b]
puts c[:missing]
p c
puts c.class
__END__
[1, 2, 3]
3
3
3
Stack
Stack
[10, 20, 40]
Array
true
true
18
HI!
Tag
true
true
true
42
2
1
0
{a: 2, b: 1}
Counter
