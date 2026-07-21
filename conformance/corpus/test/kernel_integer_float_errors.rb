# Kernel#Integer / #Float raise TypeError on nil (not 0), and Integer with a
# base and a non-String value raises ArgumentError (#2514, #2515).
p((Integer(nil) rescue $!.class))
p((Float(nil) rescue $!.class))
p((Integer(5, 16) rescue $!.class))
p(Integer("5", 16))
p(Integer("101", 2))
p(Integer("42"))
p(Float("3.5"))
p(Integer(3.9))
p(Float(7))
