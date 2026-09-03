# .freeze / .frozen? -- shallow freezing with catchable FrozenError

a = [1, 2, 3]
puts a.frozen?
a.freeze
puts a.frozen?
begin
  a[0] = 9
rescue FrozenError => e
  puts e.send(:message)
end
puts a[0]

# freeze returns self, repeat freeze is a no-op
names = ["a", "b"].freeze
names.freeze
puts names[0]
puts names.length

# shallow: a frozen container's elements stay mutable
inner = [1, 2]
outer = [inner]
outer.freeze
inner[0] = 99
puts inner[0]

# immediates and ranges are always frozen
puts 1.frozen?
puts :sym.frozen?
puts nil.frozen?
puts (1..5).frozen?

# frozen hash and string
h = { a: 1 }
h.freeze
begin
  h[:b] = 2
rescue FrozenError => e
  puts e.send(:message)
end

s = "abc".freeze
begin
  s[0] = "z"
rescue FrozenError => e
  puts e.send(:message)
end

# frozen objects reject ivar writes
class Counter
  def initialize
    @count = 0
  end
  def bump
    @count = @count + 1
  end
  def count
    @count
  end
end

c = Counter.new
c.bump
c.freeze
begin
  c.bump
rescue FrozenError
  puts "frozen counter"
end
puts c.count
__END__
false
true
can't modify frozen Array: [1, 2, 3]
1
a
2
99
true
true
true
true
can't modify frozen Hash: {a: 1}
can't modify frozen String: "abc"
frozen counter
1
