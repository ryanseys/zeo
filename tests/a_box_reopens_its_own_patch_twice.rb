b = Ruby::Box.new
b.eval("class Array; def self.a = 1; end")
b.eval("class Array; def self.a = 2; end")
p b.eval("Array.a")
p(begin; Array.a; rescue NoMethodError; :nome; end)
