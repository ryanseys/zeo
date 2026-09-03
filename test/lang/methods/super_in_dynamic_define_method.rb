# `super` inside a `define_method` whose NAME is not a literal symbol raises
# `NoMethodError: super called outside of method`. Only the literal form
# (`define_method(:who) { .. }`) desugars to a `DefMethod` node and so carries
# the `runtime_super_params` marker `emit_super` needs; the dynamic form stays
# an ordinary call with an ordinary block, and the block body has no method
# context to resolve against.
#
# The dynamic form is the one metaprogramming actually uses -- a name computed
# in a loop is the whole reason to reach for `define_method` -- so this is the
# shape a delegation or wrapper macro hits.
#
# Both the explicit-argument and the bare form are wrong, and differently: the
# explicit form should resolve, and the bare form should raise ruby's OWN
# RuntimeError about implicit argument passing (see
# `tests/zsuper_in_define_method.rb`), not this one.

class Base
  def who(*a) = "base#{a.inspect}"
end

name = :who

explicit = Class.new(Base) { define_method(name) { |*a| "dyn(" + super(*a) + ")" } }
begin
  p explicit.new.who(9)
rescue NoMethodError => e
  puts "explicit: #{e.class}: #{e.message}"
end

bare = Class.new(Base) { define_method(name) { super } }
begin
  p bare.new.who
rescue => e
  puts "bare: #{e.class}: #{e.message}"
end

# The LITERAL-name form is correct and must stay so.
lit = Class.new(Base) { define_method(:who) { |*a| "lit(" + super(*a) + ")" } }
p lit.new.who(1)
__END__
"dyn(base[9])"
bare: RuntimeError: implicit argument passing of super from method defined by define_method() is not supported. Specify all arguments explicitly.
"lit(base[1])"
