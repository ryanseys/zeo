class A
  def some_virtual_method
    1
  end
end

class B < A
  def initialize(arg = some_virtual_method)
    @arg = arg
  end
  def arg; @arg; end
end

class C < B
end

p C.new.arg
p B.new.arg
p C.new(7).arg
