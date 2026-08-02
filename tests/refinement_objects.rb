# `Module#refinements` answered `[]`, so `Refinement#target` had no receiver
# to be called on at all.
#
# A refine holder is an ORDINARY registered module carrying the refined
# methods, under a name the source could never write. What makes it a
# Refinement is the (module, target) pair the compiler marks it with -- the
# same pair that renders `#<refinement:String@M>` and that `#target` reads.

module Single
  refine String do
    def shout = upcase + "!"
  end
end

r = Single.refinements.first
p Single.refinements.size
p r.class
p r.target
p r.to_s
p r.inspect
p r.name
p r.ancestors
p r.instance_methods(false)
p [r.is_a?(Module), r.is_a?(Class), r.frozen?]
p Single.refinements.first.equal?(Single.refinements.first)

# The holder is no constant of the refining module, and the refined method is
# no method of the target.
p Single.constants
p Single.instance_methods(false)
p String.instance_methods.include?(:shout)

# Several blocks in one module keep source order.
module Several
  refine Array do
    def second = self[1]
  end
  refine Hash do
    def only = values.first
  end
  refine Symbol do
    def twice = "#{self}#{self}"
  end
end
p Several.refinements.map(&:target)
p Several.refinements.map(&:to_s)
p Several.refinements.map { |m| m.instance_methods(false) }

# A nested target prints its full path.
module Deep
  refine Process::Tms do
    def spent = utime + stime
  end
end
p Deep.refinements.map(&:target)
p Deep.refinements.map(&:to_s)

# A module with no `refine` has none, and `Refinement` itself declares one row.
p Module.new.refinements
p Comparable.refinements
p Refinement.instance_methods(false).sort
p Refinement.superclass
p Refinement.ancestors
p Refinement.instance_method(:target).arity
p Module.instance_method(:refinements).arity

# And the refinements still WORK, which is the point of all of it.
using Several
p [1, 2, 3].second
p({ a: 1 }.only)
p :ab.twice
