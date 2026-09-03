# In-body (bare and self), external constant receiver (with a param),
# and a namespaced receiver all define a class method.

class C
  define_singleton_method(:a) { "a" }
  self.define_singleton_method(:b) { "b" }
end
C.define_singleton_method(:c) { |n| n + 1 }
module M
  class D; end
end
M::D.define_singleton_method(:d) { "nested" }
puts C.a
puts C.b
puts C.c(41)
puts M::D.d
__END__
a
b
42
nested
