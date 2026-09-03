# `define_method(:name, &:other)`. In CRuby the `&` conversion happens at
# the CALL SITE, before rb_mod_define_method runs (proc.c:2872, which
# rejects a bare Symbol as its second positional arg), so the body is the
# symbol proc `->(recv, *rest) { recv.other(*rest) }` -- which is why the
# defined method takes its RECEIVER as the first argument.

class Widget
  define_method(:as_str, &:to_s)
end
class Gadget
  define_method :label, &:to_s
end
puts Widget.new.as_str(7)
puts Gadget.new.label(8)
p [1, 2, 3].map(&:to_s)
p [10, 20, 30].inject(0, &:+)
__END__
7
8
["1", "2", "3"]
60
