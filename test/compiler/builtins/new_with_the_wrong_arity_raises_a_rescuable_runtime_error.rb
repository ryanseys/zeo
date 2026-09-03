# Was a COMPILE-TIME panic, from `.new`'s own hand-rolled argument
# binding. Now that `.new` binds through the same call-argument
# lowering every other call site uses, it inherits that posture,
# which is real Ruby's: arity resolves at RUNTIME, the error is
# rescuable, and a never-executed bad call compiles fine. Both lines
# oracle-verified, message included.

class Bag
  def initialize(a, b = 1); end
end
begin
  Bag.new(1, 2, 3)
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
def never_called
  Bag.new(1, 2, 3)
end
puts "compiled fine"
__END__
ArgumentError: wrong number of arguments (given 3, expected 1..2)
compiled fine
