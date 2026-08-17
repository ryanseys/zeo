class Attr
  attr_accessor :x
  def initialize(v)
    @x = v
  end
end

class Child < Attr
  def x
    @x.length
  end
end

puts Child.new('abc').x

class IntAttr
  attr_reader :n
  def initialize(n)
    @n = n
  end
end

class Formatted < IntAttr
  def n
    "n=#{@n}"
  end
end

puts Formatted.new(7).n
puts IntAttr.new(7).n

module Sized
  attr_reader :w
end

class Box
  include Sized
  def initialize(w)
    @w = w
  end
  def w
    "#{@w}px"
  end
end

puts Box.new(12).w
