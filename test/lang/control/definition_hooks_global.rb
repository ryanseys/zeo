# A definition hook written on `Module`, `Class` or `BasicObject` ITSELF
# answers for every class in the program.
#
# No per-class owner scan can see one: such a reopen registers an ordinary
# instance method whose owner is the very class the no-op default lives on, so
# the two are indistinguishable by owner. The compiler records these by name
# instead, and hands the list to the runtime.

class Module
  def method_added(n) = puts("GLOBAL #{self}##{n}")
end

class Q
  def qq; end
end

module R
  def rr; end
end

class BasicObject
  def singleton_method_added(n) = ::Kernel.puts("GLOBAL s #{n}")
end

class S
  def self.sm; end
end

# The runtime shapes reach the same list.
Class.new { define_method(:runtime_one) { } }
Object.new.define_singleton_method(:runtime_two) { }
__END__
GLOBAL Module#method_added
GLOBAL Q#qq
GLOBAL R#rr
GLOBAL BasicObject#singleton_method_added
GLOBAL s sm
GLOBAL #<Class:0xADDR>#runtime_one
GLOBAL s runtime_two
