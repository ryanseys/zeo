# `private_constant :Hidden` as the last statement of a module body is where
# rspec-openapi and three other gems put it -- and a module body is an
# expression, so something can read what it evaluates to. The directive
# itself is a compile-time fact with no runtime emission; its VALUE is the
# module it hid the constant on (oracle-checked).
module M
  Hidden = 1
  private_constant :Hidden
end
p M.constants
v = module N
  Inner = 2
  private_constant :Inner
end
p v
class K
  Secret = 3
  private_constant :Secret
end
p K.constants
begin
  M::Hidden
rescue NameError => e
  p e.class
end
puts "still running"
__END__
[]
N
[]
NameError
still running
