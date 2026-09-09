# Modules as values and in the ancestor chain: a module constant in value
# position, `ancestors` weaving included modules in reverse-include order
# ahead of the parent chain, `<=` against a module, and `==` never holding
# between a class and a module.

module Greeting
  def hello
    "hello"
  end
end

module Politeness
  def please
    "please"
  end
end

class Base
end

class Polite < Base
  include Greeting
  include Politeness
end

# Module name as value -- compiles to sp_Class with unified cls_id.
m = Greeting
puts m.to_s          # => Greeting

# Ancestors include the modules in include-reverse order.
def names_of(arr)
  out = ""
  arr.each do |klass|
    out += "," unless out.length == 0
    out += klass.to_s
  end
  out
end

puts names_of(Polite.ancestors)
# => Polite,Politeness,Greeting,Base,Object,Kernel,BasicObject

# <= check on a module via the ancestors table.
puts (Polite <= Greeting) ? "polite<=Greeting" : "polite!<=Greeting"
puts (Polite <= Politeness) ? "polite<=Politeness" : "polite!<=Politeness"
puts (Base <= Greeting) ? "base<=Greeting" : "base!<=Greeting"

# Equality: a class is never == its included module.
puts (Polite == Greeting) ? "eq" : "neq"
__END__
Greeting
Polite,Politeness,Greeting,Base,Object,Kernel,BasicObject
polite<=Greeting
polite<=Politeness
base!<=Greeting
neq
