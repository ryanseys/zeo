# The Box docs' motivating example shape: a fresh method on `class String`,
# dispatching statically ("".blank?), dynamically through a Poly ivar
# (@s.blank?), and calling a NATIVE builtin method (`length`) implicitly on
# self from inside the reopen.

class String
  def blank?
    length == 0
  end
end

class Foo
  def initialize(s)
    @s = s
  end

  def foo_is_blank?
    @s.blank?
  end
end

puts "".blank?
puts "  hi".blank?
puts Foo.new("").foo_is_blank?
puts Foo.new("x").foo_is_blank?
__END__
true
false
true
false
