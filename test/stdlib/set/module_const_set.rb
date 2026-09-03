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
__END__
99
99
"zz"
"zz"
2.5
2.5
20
99
7
7
#@ stderr
stdlib/set/module_const_set.rb:6: warning: already initialized constant Box::X
stdlib/set/module_const_set.rb:5: warning: previous definition of X was here
stdlib/set/module_const_set.rb:8: warning: already initialized constant Box::S
stdlib/set/module_const_set.rb:5: warning: previous definition of S was here
stdlib/set/module_const_set.rb:10: warning: already initialized constant Box::F
stdlib/set/module_const_set.rb:5: warning: previous definition of F was here
stdlib/set/module_const_set.rb:13: warning: already initialized constant Y
stdlib/set/module_const_set.rb:12: warning: previous definition of Y was here
stdlib/set/module_const_set.rb:16: warning: already initialized constant Box::X
stdlib/set/module_const_set.rb:6: warning: previous definition of X was here
