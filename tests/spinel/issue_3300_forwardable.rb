require "forwardable"

class Wrapped
  def initialize
    @items = []
  end

  def add(x)
    @items << x
    self
  end

  def size
    @items.size
  end

  def label(prefix, sep)
    "#{prefix}#{sep}#{@items.size}"
  end
end

class Inner
  def greet
    "hi"
  end
end

class MyClass
  extend Forwardable

  def_delegator :@foo, :to_s, :value
  def_delegator :inner, :greet
  def_delegators :@w, :add, :size, :label

  def initialize
    @foo = 6
    @w = Wrapped.new
  end

  def inner
    @inner ||= Inner.new
  end
end

m = MyClass.new
raise "FAIL1" unless m.value == "6"
raise "FAIL2" unless m.value(2) == "110"
m.add(10)
m.add(20)
raise "FAIL3" unless m.size == 2
raise "FAIL4" unless m.label("n", ":") == "n:2"
raise "FAIL5" unless m.greet == "hi"
puts "ok"
