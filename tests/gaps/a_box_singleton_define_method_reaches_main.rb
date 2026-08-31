b = Ruby::Box.new
b.eval("Array.singleton_class.define_method(:dyn) { 42 }")
p Array.respond_to?(:dyn)
