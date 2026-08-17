# `super()` from an initialize no ancestor defines reaches Object's, which
# takes no arguments and does nothing. It was answered with a NoMethodError.

class Base
  def run(i)
    i
  end
end

class Child < Base
  def initialize(n = 3)
    super()
    @n = n
  end

  def val
    @n
  end
end

class Child2 < Base
  def initialize
    super
    @m = 1
  end

  def val
    @m
  end
end

p Child.new.val
p Child.new(9).val
p Child2.new.val
