# `Kernel#inspect` and `Kernel#to_s` AS METHOD BODIES describe the object
# -- CRuby's `rb_obj_inspect` and `rb_any_to_s`. They must not ask the
# receiver, or a `super` from an override re-enters the override.
#
# The generic renderer behind `p`, interpolation and container elements
# DOES ask, which is why the two have to be separate: the rows below pin
# both halves at once.

class Widget
  def inspect = "W:" + super
end
puts Widget.new.inspect.start_with?("W:#<Widget:0x")

class Gadget
  def to_s = "G:" + super
end
puts Gadget.new.to_s.start_with?("G:#<Gadget:0x")

# The default inspect still lists the ivars, through super as well.
class Boxed
  def initialize = @n = 7
  def inspect = "B" + super
end
puts Boxed.new.inspect.include?("@n=7")

# The generic side: `p` and interpolation ask the override, and a
# container asks it per element.
class Loud
  def inspect = "LOUD"
  def to_s = "loud"
end
p Loud.new
puts "#{Loud.new}"
p [Loud.new]

# A Struct's members survive the same path.
S = Struct.new(:a)
class S
  def inspect = "S:" + super
end
puts S.new(1).inspect
__END__
true
true
true
LOUD
loud
[LOUD]
S:#<struct S a=1>
