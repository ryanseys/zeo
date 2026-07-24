# Test include (mixin)

module Printable
  def to_str
    "Printable"
  end

  def print_info
    puts to_str
  end
end

module Measurable
  def measure
    42
  end
end

class Widget
  include Printable
  include Measurable

  def initialize(name)
    @name = name
  end

  def name
    @name
  end

  def to_str
    @name
  end
end

w = Widget.new("button")
puts w.name         # button
w.print_info        # button (uses Widget#to_str, not Printable#to_str)
puts w.measure      # 42 (from Measurable)

class Gadget
  include Printable

  def initialize(label)
    @label = label
  end

  def label
    @label
  end
end

g = Gadget.new("dial")
g.print_info        # Printable (uses Printable#to_str since Gadget doesn't override)
