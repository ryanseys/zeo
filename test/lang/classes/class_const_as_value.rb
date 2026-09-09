# A class constant in value position rather than as the receiver of `.new`:
# held in a local, an ivar and a hash value, and read back with `.to_s`.

class Row
  attr_accessor :x
end

class Article < Row
end

# Local assignment from a class constant.
c = Row
puts c.to_s                   # Row

# Same shape with a subclass.
d = Article
puts d.to_s                   # Article

# Pass a Class value through a typed-pointer round trip via an
# instance method that reads it off an ivar -- exercises the
# "store sp_Class on the heap and read it back" path that the
# bare `Foo` reference originally broke at the C-emit boundary.
class Holder
  attr_accessor :klass
end

h = Holder.new
h.klass = Row
puts h.klass.to_s             # Row
h.klass = Article
puts h.klass.to_s             # Article
__END__
Row
Article
Row
Article
