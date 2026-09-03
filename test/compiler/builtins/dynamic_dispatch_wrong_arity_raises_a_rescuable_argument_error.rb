# `send` routes through `emit_dynamic_trampoline`, whose arity guard used
# to `panic!` -- a process abort where Ruby raises a RESCUABLE
# ArgumentError. Now it emits `return Err(raise_error("ArgumentError",
# ...))`, so the program survives every mismatch and the final line
# prints. All three message shapes -- fixed `N`, range `N..M`, and the
# rest form `N+` -- are oracle-verified against ruby 4.0.6.

class A
  def fixed(a) = a
  def rng(a, b = 2) = a + b
  def restp(a, *b) = a
end
o = A.new
[[:fixed, []], [:fixed, [1, 2]], [:rng, []], [:rng, [1, 2, 3]], [:restp, []]].each do |m, args|
  begin
    o.send(m, *args)
  rescue ArgumentError => e
    puts "#{m}: #{e.message}"
  end
end
puts "survived"
__END__
fixed: wrong number of arguments (given 0, expected 1)
fixed: wrong number of arguments (given 2, expected 1)
rng: wrong number of arguments (given 0, expected 1..2)
rng: wrong number of arguments (given 3, expected 1..2)
restp: wrong number of arguments (given 0, expected 1+)
survived
