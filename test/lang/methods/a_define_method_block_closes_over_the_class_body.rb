# `define_method(:m) { ... }` in a class or module body desugars to a plain
# method -- and a method is a Rust function of its own, so it cannot reach a
# local living on the enclosing class-body frame. rubygems writes exactly that
# shape in `core_ext/kernel_warn.rb`:
#
#   module Kernel
#     original_warn = instance_method(:warn)
#     module_function define_method(:warn) { |*m, **kw| original_warn.bind_call(self, *m, **kw) }
#   end
#
# which emitted a method body naming `original_warn` and stopped bundler and
# rubyzip at rustc with E0425. A capturing block declines the desugar and
# installs a real closure through `Module#define_method` instead.

module Greeter
  greeting = "hi"
  define_method(:hello) { greeting }
end
class Plain
  include Greeter
end
p Plain.new.hello

class Shouter
  suffix = "!"
  # A parameter of its own alongside the captured local.
  define_method(:shout) { |word| [word, suffix].join }
  # Captured from one scope further in, so the depth is 2 rather than 1.
  define_method(:nested) { [1].map { suffix }.first }
  # No capture at all: this one keeps the direct desugar, and its own locals
  # must not be mistaken for an enclosing scope's.
  define_method(:own) { |a| doubled = a * 2; doubled + 1 }
end
p Shouter.new.shout("hey")
p Shouter.new.nested
p Shouter.new.own(4)

# The captured local is shared, not copied: a later write in the class body is
# visible to the method, exactly as a closure requires.
class Counter
  n = 0
  define_method(:bump) { n += 1 }
  define_method(:read) { n }
end
c = Counter.new
c.bump
c.bump
p c.read

# A `define_method` inside a METHOD body was always a closure and must stay
# one -- the desugar there emits the def in expression position.
class Later
  def self.build(v)
    define_method(:val) { v }
  end
  build(42)
end
p Later.new.val
__END__
"hi"
"hey!"
"!"
9
2
42
