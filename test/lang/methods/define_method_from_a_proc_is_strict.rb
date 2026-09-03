# A method defined from a plain proc still checks METHOD arity when called
# (`define_method` hardens proc semantics); zeo keeps the proc's lenient
# binding and pads with nil. The lambda-sourced form already agrees.
# (Found by the 2026-08-24 probe sweep.)
c = Class.new
c.send(:define_method, :two) { |a, b| [a, b] }
begin
  p c.new.two(1)
rescue ArgumentError => e
  puts e.message
end
p c.new.two(1, 2)
__END__
wrong number of arguments (given 1, expected 2)
[1, 2]
