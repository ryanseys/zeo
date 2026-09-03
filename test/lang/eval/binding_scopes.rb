# Where a `binding` is taken decides what it holds. A method's carries its
# parameters (every kind) ahead of its locals; a class body's `self` and cref
# are the class, which is what makes the `eval <<-RUBY, binding` idiom every
# gemspec attribute writer is built from work; a block's names come first and
# the enclosing scope's after, innermost outward.

puts "-- a method frame --"
def m(a, b = 2, *rest, k: 3, **kw, &blk)
  c = a + b
  bd = binding
  p bd.local_variables
  p [bd.local_variable_get(:a), bd.local_variable_get(:b)]
  p [bd.local_variable_get(:rest), bd.local_variable_get(:k)]
  p [bd.local_variable_get(:kw), bd.local_variable_get(:blk)]
  bd
end
mb = m(1)
p mb.local_variable_get(:c)
p mb.eval("a + c")

puts "-- an instance method: self, ivars, lexical constants --"
class Holder
  LIMIT = 5
  def initialize
    @v = 10
  end

  def peek
    y = 1
    binding
  end
end
hb = Holder.new.peek
p hb.receiver.class
p hb.local_variables
p [hb.eval("@v"), hb.eval("y"), hb.eval("LIMIT")]

puts "-- a class body, and a def written through its binding --"
class Built
  cb = binding
  p cb.receiver
  p cb.local_variables

  eval <<-RUBY, binding, __FILE__, __LINE__ + 1
    def greet
      "hi from eval"
    end
    private :greet
  RUBY

  WIDTH = 3
  p cb.eval("WIDTH")
end
p Built.new.send(:greet)
p Built.private_instance_methods(false)

puts "-- a block: its own names first, then the enclosing scope's --"
def outer
  o = 1
  [1].each do |i|
    inner = i + o
    return binding
  end
end
ob = outer
p ob.local_variables
p [ob.local_variable_get(:i), ob.local_variable_get(:inner), ob.local_variable_get(:o)]
ob.local_variable_set(:o, 100)
p ob.eval("inner + o")

puts "-- nested blocks compose --"
def deep
  q = 1
  [1].map { |i| [2].map { |j| binding } }
end
db = deep[0][0]
p db.local_variables.sort
p [db.local_variable_get(:q), db.local_variable_get(:i), db.local_variable_get(:j)]
__END__
-- a method frame --
[:a, :b, :rest, :k, :kw, :blk, :c, :bd]
[1, 2]
[[], 3]
[{}, nil]
3
4
-- an instance method: self, ivars, lexical constants --
Holder
[:y]
[10, 1, 5]
-- a class body, and a def written through its binding --
Built
[:cb]
3
"hi from eval"
[:greet]
-- a block: its own names first, then the enclosing scope's --
[:i, :inner, :o]
[1, 2, 1]
102
-- nested blocks compose --
[:i, :j, :q]
[1, 1, 2]
