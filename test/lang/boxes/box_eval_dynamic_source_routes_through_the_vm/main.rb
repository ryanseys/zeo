box = Ruby::Box.new
box.require_relative "lib"
code = "1 + 2"
p box.eval(code)
p box.eval("WIDGET_CONST")
p box.eval("Widget.describe")
$g = "main value"
read = "$g"
p box.eval(read)
box.eval("$g = 'box value'")
p $g
p box.eval(read)
begin
  box.eval(123)
rescue TypeError => e
  puts "rescued: #{e.message}"
end
