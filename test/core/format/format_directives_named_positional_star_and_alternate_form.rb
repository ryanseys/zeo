puts format("%<name>s is %<age>d", name: "Ada", age: 36)
puts format("%#b / %#x", 10, 255)
puts format("%*d|", 5, 42)
puts format("%2$s %1$s", "world", "hello")
puts format("%a", 0.5)
puts("%<x>05.2f" % { x: 3.14159 })
__END__
Ada is 36
0b1010 / 0xff
   42|
hello world
0x1p-1
03.14
