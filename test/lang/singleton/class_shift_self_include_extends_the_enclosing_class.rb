# `include M` inside `class << self` == `extend M` on the enclosing class:
# the module's instance methods become the class's class methods.

module Greeter
  def hi(n); "hi #{n}"; end
end
class Widget
  class << self
    include Greeter
  end
end
puts Widget.hi("bob")
__END__
hi bob
