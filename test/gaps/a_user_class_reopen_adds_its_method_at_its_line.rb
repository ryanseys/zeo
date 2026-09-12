# A `def` a user class REOPEN adds, probed above the reopen.
class Foo
end
p Foo.method_defined?(:a), Foo.respond_to?(:b)
class Foo
  def a = 1
  def self.b = 2
end
p Foo.method_defined?(:a), Foo.respond_to?(:b)
__END__
false
false
true
true
