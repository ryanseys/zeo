# A constructor's default argument is evaluated on the object being built, so
# it can call the object's own methods. Emitting it at the call site named a
# `self` that does not exist there yet; the construction now allocates first
# and runs initialize with the fresh object in scope.

class A
  def base
    10
  end

  def initialize(n = base * 2)
    @n = n
  end

  def val
    @n
  end
end

class Counter
  attr_reader :size
  def initialize(items = default_items)
    @size = items.length
  end

  def default_items
    [1, 2, 3]
  end
end

p A.new.val
p A.new(5).val
p Counter.new.size
p Counter.new([1]).size
