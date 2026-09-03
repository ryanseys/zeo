#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("module Kernel; def kmeth = 1; end")
p(begin; kmeth; rescue NoMethodError, NameError; :nome; end)
p b.eval("kmeth")
__END__
:nome
1
