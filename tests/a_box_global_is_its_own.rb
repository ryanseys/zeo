b = Ruby::Box.new
b.eval("$boxg = 7")
p $boxg
p b.eval("$boxg")
