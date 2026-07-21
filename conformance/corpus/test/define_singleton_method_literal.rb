class C
  define_singleton_method(:hello) { "hi" }                 # no receiver: the enclosing class
  self.define_singleton_method(:hello_self) { "hi self" }  # self receiver
end

C.define_singleton_method(:world) { "earth" }              # class-constant receiver
C.define_singleton_method(:double) { |x| x * 2 }           # parameterized

module M
  class D
  end
end
M::D.define_singleton_method(:nested) { "nested" }         # namespaced (ConstantPath) receiver

puts C.hello
puts C.hello_self
puts C.world
puts C.double(21)
puts M::D.nested
