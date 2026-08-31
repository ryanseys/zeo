b = Ruby::Box.new
b.eval("class Owned; def self.go = 'go'; def inst = 'i'; end")
p b::Owned.go
p b::Owned.new.inst
p b::Owned.respond_to?(:go)
