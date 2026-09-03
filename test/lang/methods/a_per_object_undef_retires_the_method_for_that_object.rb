# `undef_method` through an OBJECT's singleton class retires the name for that
# one object. zeo recorded nothing: the class still defined the name, so every
# lookup found it and the call answered where ruby raises.
#
# A tombstone, not an absence -- there is no method to remove, only a class
# definition to stop answering with.
class Foo
  def close = "class"
  def read = "read"
end

g = Foo.new
g.singleton_class.undef_method(:close)
p g.respond_to?(:close)
p(begin
  g.close
rescue NoMethodError
  :raised
end)

# The retirement is that object's alone, and reaches only the name it named.
p Foo.new.close
p g.read

# It outranks a singleton method of the same name installed BEFORE it, and the
# object can be given a fresh one afterwards.
h = Foo.new
h.define_singleton_method(:close) { "mine" }
p h.close
h.singleton_class.undef_method(:close)
p(begin
  h.close
rescue NoMethodError
  :raised
end)
h.define_singleton_method(:close) { "again" }
p h.close

# `method_missing` gets its turn, exactly as it does for a name nothing ever
# defined.
class Catcher
  def gone = "still here"
  def method_missing(n, *) = "missing:#{n}"
  def respond_to_missing?(_n, _priv = false) = true
end
c = Catcher.new
p c.gone
c.singleton_class.undef_method(:gone)
p c.gone
__END__
false
:raised
"class"
"read"
"mine"
:raised
"again"
"still here"
"missing:gone"
