# `UnboundMethod#bind` asks whether the argument is an instance of the method's
# OWNER, and a class method's owner is `#<Class:Foo>` -- so the argument must be
# a class in Foo's ancestry, not an instance of Foo. CRuby words that refusal
# differently too, because it reaches the test through a singleton `methclass`.

class Foo
  def self.a = 1
  def b = 2
end
class Sub < Foo; end
class Other; end

um = Foo.method(:a).unbind
p um.owner
p um.bind(Foo).call

# A subclass IS an instance of `#<Class:Foo>`, so it binds and answers.
p um.bind(Sub).call

# Everything else is refused with the singleton wording -- an unrelated class,
# an instance of the owner, and an immediate all take the same message.
[Other, Foo.new, 3].each do |bad|
  begin
    um.bind(bad)
  rescue TypeError => e
    puts e.message
  end
end

# `bind_call` runs the same check.
p um.bind_call(Sub)
begin
  um.bind_call(Other)
rescue TypeError => e
  puts e.message
end

# Reaching the same method through the singleton class gives an INSTANCE-kind
# unbound method, and it binds the same way.
sm = Foo.singleton_class.instance_method(:a)
p sm.owner
p sm.bind(Foo).call

# The instance case is unaffected, which is what makes this the singleton branch
# rather than `bind` as a whole.
p Foo.instance_method(:b).bind(Foo.new).call
p Foo.instance_method(:b).bind(Sub.new).call

# ...and a genuinely wrong argument must still be refused, with the wording the
# non-singleton owner gets.
begin
  Foo.instance_method(:b).bind(Object.new)
rescue TypeError => e
  puts e.message
end
__END__
#<Class:Foo>
1
1
singleton method called for a different object
singleton method called for a different object
singleton method called for a different object
1
singleton method called for a different object
#<Class:Foo>
1
2
2
bind argument must be an instance of Foo
