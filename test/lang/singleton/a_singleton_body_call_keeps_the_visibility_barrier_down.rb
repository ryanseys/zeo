# Every shape where a `class << X` body's receiverless call reaches a method
# that is private on the singleton class. Ruby runs those with no receiver, so
# no visibility barrier applies -- zeo rebinds them onto a synthesized
# `X.singleton_class` and must keep the barrier down.

module Priv
  def inst; "inst"; end
  def other; "other"; end
end

def try(tag)
  puts "#{tag}: #{yield.inspect}"
rescue StandardError => e
  puts "#{tag}: #{e.class}: #{e.message}"
end

# a: `class << obj`, a private Module verb, splatted args.
A = Module.new { def a; 1; end }
class << A
  public(*[])
end
try("a") { A.singleton_class.private_method_defined?(:public) }

# b: `class << self` in a class body -- the same verb.
class B
  def self.b; 1; end
  private_class_method :b
  class << self
    public(*[:b])
  end
end
try("b") { B.b }

# c: `class << obj`, a private Module verb reached receiverlessly.
#    `define_method` is private on Module, so this is the same barrier as the
#    fileutils shape with a body that proves the call ran.
C = Module.new
class << C
  define_method(:c1) { "c1" }
end
try("c") { C.c1 }

# d: an explicit literal `self` receiver -- allowed since ruby 2.7.
D = Module.new
class << D
  self.define_method(:d1) { "d1" }
end
try("d") { D.d1 }

# e: a receiver the SOURCE wrote must STILL raise. `public` stayed private on
#    Module; `define_method` did not, which is why it is the one used above.
E = Module.new
try("e") { E.singleton_class.public(*[]) }

# f: `module_function`-shaped -- private instance methods made public class
#    methods from the singleton body, which is what fileutils does.
module F
  include Priv
  private(*[:inst, :other])
  extend self
  class << self
    public(*[:inst, :other])
  end
end
try("f") { [F.inst, F.singleton_class.public_method_defined?(:inst)] }

# g: the same through an eval, where the class body is re-sliced from source.
eval(%q{
  module G
    include Priv
    private(*[:inst])
    extend self
    class << self
      public(*[:inst])
    end
  end
})
try("g") { [G.inst, G.singleton_class.public_method_defined?(:inst)] }

# h: a private verb reached from inside a BLOCK in the singleton body.
module H
  def self.h1; 1; end
  private_class_method :h1
  class << self
    [:h1].each { |n| public(*[n]) }
  end
end
try("h") { H.singleton_class.public_method_defined?(:h1) }
__END__
a: true
b: 1
c: "c1"
d: "d1"
e: NoMethodError: private method 'public' called for class #<Class:E>
f: ["inst", true]
g: ["inst", true]
h: true
