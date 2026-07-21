# Three-level inheritance (Base -> Child -> GrandChild) plus a
# Sibling. All inherit #greet from Base without overriding it.
# Calling via a poly-typed parameter must dispatch correctly for
# every level in the chain.

class Base
  def greet
    "hello"
  end
end

class Child < Base
end

class GrandChild < Child
end

class Sibling < Base
end

def call_greet(e)
  e.greet
end

puts call_greet(Base.new)
puts call_greet(Child.new)
puts call_greet(GrandChild.new)
puts call_greet(Sibling.new)
