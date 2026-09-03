class Widget
  def hi
    "main widget"
  end
end
box = Ruby::Box.new
box.require_relative "widget"
p Widget.new.hi
w = box::Widget.new
p w.hi
p box::Widget == Widget
p box::String == String
p box::WIDGET_CONST
p $box_g
p box
