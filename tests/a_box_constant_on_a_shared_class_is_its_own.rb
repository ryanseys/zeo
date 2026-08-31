b = Ruby::Box.new
b.eval("class Array; BOXC = 11; end")
p(begin; Array::BOXC; rescue NameError; :namee; end)
p b.eval("Array::BOXC")
