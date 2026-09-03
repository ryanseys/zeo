# `Class.new(Super) { ... }` mints a runtime class. The block body
# runs against the new class (`define_method` and `def` both install on
# it); a constant binding names it; is_a?/instance_of?/superclass and a
# runtime superclass chain all resolve.

Widget = Class.new do
  define_method(:kind) { "widget" }
  def size
    10
  end
end
w = Widget.new
puts w.kind
puts w.size
puts w.is_a?(Widget)
puts Widget.name
Gadget = Class.new(Widget) do
  define_method(:extra) { "gadget" }
end
g = Gadget.new
puts g.kind
puts g.extra
puts g.is_a?(Widget)
puts g.instance_of?(Widget)
puts Gadget.superclass.name
__END__
widget
10
true
Widget
widget
gadget
true
false
Widget
