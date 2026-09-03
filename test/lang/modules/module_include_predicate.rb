module Greetable; end
class Person; include Greetable; end
p Person.include?(Greetable)
p Person.include?(Comparable)
begin
  Person.include?(Object)
rescue TypeError => e
  puts e.message
end
__END__
true
false
wrong argument type Class (expected Module)
