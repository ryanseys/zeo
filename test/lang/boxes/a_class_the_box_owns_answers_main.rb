# The OTHER direction, and the one a strict reading of the rule gets
# wrong: a class the box DEFINED is the box's own class, so its methods
# answer every caller, main included. Isolation is a property of a shared
# class the box patched, never of a class it owns outright.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("class Owned; def self.go = 'go'; def inst = 'i'; end")
p b::Owned.go
p b::Owned.new.inst
p b::Owned.respond_to?(:go)
__END__
"go"
"i"
true
