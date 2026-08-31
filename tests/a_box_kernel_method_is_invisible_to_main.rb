b = Ruby::Box.new
b.eval("module Kernel; def kmeth = 1; end")
p(begin; kmeth; rescue NoMethodError, NameError; :nome; end)
p b.eval("kmeth")
