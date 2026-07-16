# `define_singleton_method` adds a method to a single class's own singleton --
# in practice, a class method. It works from inside the class body and from
# outside on a class constant, including a namespaced one.
class Greeter
  define_singleton_method(:hello) { "hi" }
  self.define_singleton_method(:hey) { "hey there" }
end

# From outside, on the class constant -- with parameters, too.
Greeter.define_singleton_method(:shout) { |name| "HELLO, #{name.upcase}" }

module Outer
  class Inner
  end
end
Outer::Inner.define_singleton_method(:whoami) { "Outer::Inner" }

puts Greeter.hello                    # hi
puts Greeter.hey                      # hey there
puts Greeter.shout("ada")             # HELLO, ADA
puts Outer::Inner.whoami              # Outer::Inner
