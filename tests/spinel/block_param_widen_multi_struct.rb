# A block over an unresolved receiver, for a name the builtin Enumerable
# surface also owns, must have its params widened to poly: the dispatch can
# reach a container at run time, and a user method's yield types describe only
# the user arm. That widening was tied to a class having been ADOPTED, which
# happens only when EXACTLY ONE user class defines the name. Every Struct
# defines `each`, so a second Struct in the file meant nothing was adopted, the
# widening was skipped, and the param kept an earlier round's guess -- Integer.
# The Struct members typed from it followed, and the C build stopped (#4086).
#
# More candidates is a stronger case for poly, not a weaker one. The widening
# now needs a candidate to exist, not to be unique.

module Probe
  Disallowed = Struct.new(:term)
  Finding = Struct.new(:path, :term)

  def self.check(scope)
    scope.keys.each { |a| [1].each { |b| Finding.new(a, a) } }
  end
end

f = Probe::Finding.new("a.rb", "Order")
puts "#{f.path} #{f.term}"

# and the method actually called (its RETURN is a separate gap, on master too:
# `each` through a module method answers nil rather than the receiver, so what
# is checked here is that calling it works and the members stay right)
Probe.check({ "x" => 1 })
p Probe::Finding.new("b.rb", "Term").path

# one Struct, which is the shape that worked by accident
module Solo
  Only = Struct.new(:v)

  def self.walk(scope)
    scope.keys.each { |a| [1].each { |b| Only.new(a) } }
  end
end
p Solo::Only.new("s").v
Solo.walk({ "y" => 2 })

# three, to show it is not about the count
module Trio
  A = Struct.new(:x)
  B = Struct.new(:y)
  C = Struct.new(:z)

  def self.walk(scope)
    scope.each { |a| [1].each { |b| C.new(a) } }
  end
end
p Trio::C.new("t").z
Trio.walk(["u"])

# a name no user class defines still types its param from the receiver: this is
# what over-widening broke (a Range element is an Integer, not a poly)
total = 0
(1..3).each { |x| total += x }
p total
p (1..3).map { |x| x * 2 }
p [1, 2, 3].each_cons(2).map { |pair| pair[0] + pair[1] }
