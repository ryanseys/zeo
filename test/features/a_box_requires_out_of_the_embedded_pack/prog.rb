name = ["greeter", ""].first
require name
p Greeter.hi("main")
b = Ruby::Box.new
p b.require(name)
p b.eval("Greeter.hi('box')")
