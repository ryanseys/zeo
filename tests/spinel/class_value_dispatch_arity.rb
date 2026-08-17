# Dispatch through a class-valued slot compiles to a switch over cls_id, and
# each arm calls a different C function with its own parameter list. Passing
# the caller's argument list verbatim only worked while every candidate took
# exactly as many as the call supplied (#3526), and a class with no such
# method fell out of the switch with the result slot at its default, which the
# caller could not tell from an answer (#3527).
class Holder
  def initialize(k)
    @k = k
  end

  def klass
    @k
  end

  def go1
    klass.f(1)
  end

  def go2
    klass.g(1, 2)
  end
end

class A
  def self.f(x, y = nil)
    "A.f(#{x.inspect}, #{y.inspect})"
  end

  def self.g(x, r)
    "A.g"
  end
end

class B
  def self.f(x, y = nil)
    "B.f(#{x.inspect}, #{y.inspect})"
  end

  def self.g(x, r, extra = nil)
    "B.g(extra=#{extra.inspect})"
  end
end

puts Holder.new(A).go1
puts Holder.new(B).go1
puts Holder.new(A).go2
puts Holder.new(B).go2

class NoMethods
end

begin
  Holder.new(NoMethods).go1
  puts "no raise"
rescue NoMethodError
  puts "NoMethodError"
end
