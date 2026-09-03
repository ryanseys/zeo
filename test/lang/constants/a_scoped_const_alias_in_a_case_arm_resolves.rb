# A `NAME = SomeModule` write inside a class or module body binds the name
# whatever container it is written in -- a `case` arm here, but a `begin` body
# or a block just as much. Only the `if` shape was recorded, so `include
# Ordered` below raised `uninitialized constant Ns::Uses::Ordered` where CRuby
# includes Comparable.
module Ns
  case ENV.fetch("ZEO_BASE", "a")
  when "a" then Ordered = Comparable
  else Ordered = Comparable
  end

  class Uses
    include Ordered
    def <=>(other) = 0
  end
end

p Ns::Uses.ancestors.include?(Comparable)
p Ns::Uses.new == Ns::Uses.new
__END__
true
true
