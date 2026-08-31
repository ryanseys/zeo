b = Ruby::Box.new
b.eval("Object.const_set(:BOXCONST, 5)")
p(begin; Object.const_get(:BOXCONST); rescue NameError; :namee; end)
