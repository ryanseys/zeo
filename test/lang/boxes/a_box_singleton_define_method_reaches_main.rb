#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("Array.singleton_class.define_method(:dyn) { 42 }")
p Array.respond_to?(:dyn)
__END__
false
