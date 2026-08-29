module Kernel
  def tagged(x) = x
end

class Widget; end

p Widget.new.method(:tagged).owner
p 5.method(:tagged).owner
p "s".method(:tagged).owner
p Widget.instance_method(:tagged).owner
p Integer.instance_method(:tagged).owner

p [Widget.include?(Kernel), Integer.include?(Kernel), Widget.new.is_a?(Kernel)]
p Kernel.instance_method(:tagged).owner
