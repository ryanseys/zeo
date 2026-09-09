# A subclass's singleton class inherits its parent's, so the call resolves
# through the chain.

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
__END__
"tagged"
"tagged"
"tagged"
true
true
true
"tagged"
"own"
"marked"
"marked"
false
undefined method 'tag' for class Other
