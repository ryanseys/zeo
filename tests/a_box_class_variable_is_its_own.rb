b = Ruby::Box.new
b.eval("class Array; @@cv = 3; def self.cv = @@cv; end")
p(begin; Array.cv; rescue NoMethodError; :nome; end)
p b.eval("Array.cv")
