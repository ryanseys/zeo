# Ruby lets you hang an instance variable on almost anything. An Object keeps
# its ivars in its own struct and a class keeps them in `civars`; a bare
# Array/String/Hash/Proc has neither, so zeo stores theirs in an
# identity-keyed table beside the value (`value_ivars`).
#
# The permanently-frozen tier is the only refusal, and it is a FrozenError
# rather than a silent no-op: an Integer, a Symbol, `nil`, a Range.

puts "== the heap kinds accept one"
[[], +"s", {}, proc {}, Regexp.new("x"), /x/.match("x")].each do |v|
  v.instance_variable_set(:@x, 1)
  p [v.class, v.instance_variable_get(:@x), v.instance_variables, v.instance_variable_defined?(:@x)]
end

puts "== the frozen tier raises"
[1, 1.5, :s, nil, true, (1..2), [].freeze].each do |v|
  begin
    v.instance_variable_set(:@x, 1)
  rescue => e
    puts "#{v.class}: #{e.class}"
  end
end

puts "== assignment order, and a second write keeps its place"
a = []
a.instance_variable_set(:@b, 2)
a.instance_variable_set(:@a, 1)
a.instance_variable_set(:@b, 9)
p [a.instance_variables, a.instance_variable_get(:@b)]

puts "== two values of the same kind do not share"
one = []
two = []
one.instance_variable_set(:@k, 1)
p [one.instance_variables, two.instance_variables, two.instance_variable_get(:@k)]

puts "== remove"
p a.remove_instance_variable(:@b)
p a.instance_variables
begin
  a.remove_instance_variable(:@b)
rescue NameError => e
  puts e.message
end

puts "== instance_eval and instance_exec reach the same store"
b = []
b.instance_eval { @y = 5 }
p [b.instance_variables, b.instance_eval { @y }]
b.instance_exec(7) { |n| @y = n }
p b.instance_variable_get(:@y)

puts "== an unset ivar reads nil, it does not raise"
p [[].instance_variable_get(:@nope), [].instance_eval { @nope }]

puts "== dup and clone carry them"
c = +"str"
c.instance_variable_set(:@z, 9)
p [c.dup.instance_variables, c.clone.instance_variables, c.dup.instance_variable_get(:@z)]
# The copy is its own value: writing one must not touch the other.
d = c.dup
d.instance_variable_set(:@z, 100)
p [c.instance_variable_get(:@z), d.instance_variable_get(:@z)]

puts "== the value itself is unchanged -- inspect and == ignore ivars"
p c
p [c == "str", b, b == []]

puts "== a module mixed in with extend can read them"
module Reader
  def z = @z
end
e = []
e.extend(Reader)
e.instance_variable_set(:@z, 42)
p e.z

puts "== defined?"
f = []
p defined?(f.instance_variables)
f.instance_variable_set(:@q, 1)
p f.instance_eval { defined?(@q) }
p f.instance_eval { defined?(@never) }
