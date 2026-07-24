# Parent defines attr_reader :val, SubHolder inherits it.
# Reading .val via a poly-typed parameter must dispatch
# correctly for both parent and subclass instances.

class Holder
  def initialize(v)
    @val = v
  end
  attr_reader :val
end

class SubHolder < Holder
end

def read_val(e)
  e.val
end

puts read_val(Holder.new("a"))
puts read_val(SubHolder.new("b"))
