#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
# Ruby::Box, the namespace isolation. A class defined in a
# box is a distinct class; builtins are shared (and patchable PER BOX);
# globals and top-level constants are fully box-separate; values and
# exceptions cross the boundary as plain references.

class Widget
  def hi
    "main widget"
  end
end

box = Ruby::Box.new
box.eval("class Widget; def hi; 'box widget'; end; end")

p Widget.new.hi
w = box.eval("Widget.new")
p w.hi
p box::Widget == Widget
p box::String == String

box.eval("BOX_CONST = 99")
p box::BOX_CONST

$g = "main value"
p box.eval("$g")
box.eval("$g = 'box value'")
p $g
p box.eval("$g")

box.eval("class String; def shout; upcase + '!'; end; end")
p box.eval("'hey'.shout")
begin
  "hey".shout
rescue NoMethodError
  puts "main: no shout"
end

box2 = Ruby::Box.new
box2.eval("class Widget; def hi; 'other box'; end; end")
p box2::Widget.new.hi
p(box::Widget == box2::Widget)

box.eval("class BoxError < StandardError; end")
begin
  box.eval("raise BoxError, 'crossed'")
rescue StandardError => e
  puts "rescued: #{e.message} (#{e.class})"
end
__END__
"main widget"
"box widget"
false
true
99
nil
"main value"
"box value"
"HEY!"
main: no shout
"other box"
false
rescued: crossed (BoxError)
