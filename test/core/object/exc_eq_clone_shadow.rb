# Three unrelated surfaces: Exception#== compares class and message, clone
# keeps a frozen Hash frozen where dup does not, and a module's private
# helper does not appear in an includer's method list.

# A user Exception subclass keeps Exception#== -- class plus message -- rather
# than falling back to Object#== identity
class OvB < StandardError; end
p(OvB.new("t") == OvB.new("t"))
a = OvB.new("t")
b = OvB.new("t")
p(a == b)
p(a == OvB.new("u"))
p(a == a)
p(a != b)
p(a.equal?(b))

class WithIvar < StandardError
  def initialize(m, code)
    super(m)
    @code = code
  end
  def code; @code; end
end
p(WithIvar.new("m", 1) == WithIvar.new("m", 2))
p(WithIvar.new("m", 1) == WithIvar.new("n", 1))

# Hash#clone carries the frozen flag over, Hash#dup drops it (Array and String
# already did)
h = { a: 1 }.freeze
v = h.clone
p v.frozen?
r = ((v[:b] = 2) rescue $!.class); p r
p h.dup.frozen?
p({ a: 1 }.clone.frozen?)
p [1, 2].freeze.clone.frozen?
p "ab".freeze.clone.frozen?

# The shadow copy of an included module's method (the super target) is
# implementation detail: reflection must not see it
module Inc; def tag; "inc"; end; end
class Sub
  include Inc
  def tag; "sub(" + super + ")"; end
end
p Sub.instance_methods(false)
p Sub.method_defined?(:__inc_0_tag)
p Sub.new.respond_to?(:__inc_0_tag)
p Sub.new.tag
__END__
true
true
false
true
false
false
true
false
true
FrozenError
false
false
true
true
[:tag]
false
false
"sub(inc)"
