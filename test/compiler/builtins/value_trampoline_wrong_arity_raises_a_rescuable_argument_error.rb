# The `ValueMethodFn` trampoline (`emit_value_trampoline`) backs both a
# user CLASS method reached dynamically and a reopened-builtin instance
# method. Its arity guard shared the same `panic!` bug as the dynamic
# trampoline; both now raise a rescuable ArgumentError. Oracle-verified.

class A
  def self.cm(a, b) = a + b
end
class Integer
  def double(a); self * 2; end
end
k = A
begin
  k.send(:cm, 1)
rescue ArgumentError => e
  puts "cm: #{e.message}"
end
begin
  5.send(:double)
rescue ArgumentError => e
  puts "double: #{e.message}"
end
puts "survived"
__END__
cm: wrong number of arguments (given 1, expected 2)
double: wrong number of arguments (given 0, expected 1)
survived
