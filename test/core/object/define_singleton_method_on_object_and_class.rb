# A per-object singleton (only that object responds) and a
# class-level singleton method (a class method).

class Widget; end
a = Widget.new
a.define_singleton_method(:special) { "just me" }
puts a.special
puts Widget.new.respond_to?(:special)
Widget.define_singleton_method(:factory) { "built" }
puts Widget.factory
__END__
just me
false
built
