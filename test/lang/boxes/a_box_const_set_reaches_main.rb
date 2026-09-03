#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("Object.const_set(:BOXCONST, 5)")
p(begin; Object.const_get(:BOXCONST); rescue NameError; :namee; end)
__END__
:namee
