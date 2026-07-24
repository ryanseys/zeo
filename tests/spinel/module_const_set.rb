# Module#const_set with a literal name re-assigns the constant (#2675). A
# constant is a compile-time C global, so an existing one can be stored into;
# a name the program never writes has no storage, and the global's type is
# fixed at its definition -- both of those report a specific diagnostic.
class Box; X = 1; S = "a"; F = 1.5; end
p Box.const_set(:X, 99)     # returns the value, as CRuby does
p Box::X
p Box.const_set(:S, "zz")
p Box::S
p Box.const_set(:F, 2.5)
p Box::F
Y = 10
Object.const_set(:Y, 20)
p Y
p Box.const_get(:X)         # const_get sees the store
p Box.const_set("X", 7)     # a String name works like a Symbol one
p Box::X
