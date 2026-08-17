# `table[key].new` -- the receiver is a Class value read out of a container, so
# the construction switches on its runtime id. The zero-argument form now also
# reaches a constructor whose parameters are all optional, filling each with
# its default the way a statically-known `Klass.new` does.

class A
  def initialize(n = 3)
    @n = n
  end

  def val
    @n
  end
end

class B
  def initialize(n = 7)
    @n = n
  end

  def val
    @n * 2
  end
end

table = { "a" => A, "b" => B }
["a", "b"].each do |k|
  klass = table[k]
  obj = klass.new
  p obj.val
end

p table["a"].new.val
