# Parent defines a method, child inherits it.
# Calling the method via a poly-typed parameter must dispatch
# to the parent's method body for both parent and child instances.

class A
  def x
    "from-x"
  end
end

class B < A
end

def call_x(e)
  e.x
end

puts call_x(A.new)
puts call_x(B.new)
