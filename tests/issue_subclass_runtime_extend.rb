# A subclass did not inherit class methods installed by a RUNTIME `extend`.
# `runtime_extend` writes them into the extended class's own overlay
# `class_methods`, and both the call and the reflection probed the receiver's
# own overlay alone. In ruby a subclass's singleton class INHERITS its parent's,
# so the call resolves through the chain.
#
# Only the overlay needed the walk: materialization already copies a frozen
# `def self.x` onto every subclass's registry entry, but nothing copies
# something installed at run time.
#
# The class-body form (`class Base; extend Store; end`) already worked and is
# covered by `tests/runtime_extend_binds_the_class.rb`.

module Store
  def tag = "tagged"
end

class Base; end
Base.extend Store

class Sub < Base; end
class Deep < Sub; end

p Base.tag
p Sub.tag
p Deep.tag

# The reflection has to agree with what dispatch answers.
p Sub.respond_to?(:tag)
p Deep.respond_to?(:tag)
p Sub.singleton_methods.include?(:tag)
p Sub.method(:tag).call

# A nearer OWN definition still wins -- ruby's placement rule, and the reason
# the ancestor walk runs last.
class Own < Base
  def self.tag = "own"
end
p Own.tag

# `define_singleton_method` on a superclass travels the same way.
class Root; end
Root.define_singleton_method(:mark) { "marked" }
class Leaf < Root; end
p Root.mark
p Leaf.mark

# An unrelated class sees none of it.
class Other; end
p Other.respond_to?(:tag)
begin
  Other.tag
rescue NoMethodError => e
  puts e.message
end
