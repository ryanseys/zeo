# A module reopened inside a box keeps its methods there -- `Kernel` is the
# sharpest case, since a leak would give every object in main a new method.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("module Kernel; def kmeth = 1; end")
p(begin; kmeth; rescue NoMethodError, NameError; :nome; end)
p b.eval("kmeth")
__END__
:nome
1
