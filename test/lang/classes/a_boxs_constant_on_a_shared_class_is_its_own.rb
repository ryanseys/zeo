# A constant written on a SHARED class inside a box, read back by path.
# `Array::BOXC` is a scope-operator read rather than a cref walk, so it is
# a different path into the same table.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("class Array; BOXC = 11; end")
p(begin; Array::BOXC; rescue NameError; :namee; end)
p b.eval("Array::BOXC")
__END__
:namee
11
