module PB; def inspect = "P" + super; end
class Array; prepend PB; end
p [1, 2].inspect

class Integer
  def inspect = "I" + super
end
p 5.inspect
