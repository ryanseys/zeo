require 'set'

# A frozen Set refuses every mutation, as CRuby's does. The element array is a
# private ivar, so freezing the Set itself was invisible to the mutators and
# every one of them went through.

a = Set[1, 2].freeze
p((a.add(3) rescue $!.class))
p((a << 4 rescue $!.class))
p((a.delete(1) rescue $!.class))
p((a.merge([9]) rescue $!.class))
p((a.subtract([2]) rescue $!.class))
p((a.replace([7]) rescue $!.class))
p(a.to_a)
p((a.clear rescue $!.class))
p((a.add?(5) rescue $!.class))
p((a.delete?(1) rescue $!.class))
p((a.keep_if { |x| x > 1 } rescue $!.class))
p((a.delete_if { |x| x > 1 } rescue $!.class))
p((a.map! { |x| x } rescue $!.class))
p a.to_a
p a.frozen?

# an unfrozen Set still mutates
b = Set[1, 2]
b.add(3)
b << 4
b.delete(1)
p b.to_a
p b.frozen?
