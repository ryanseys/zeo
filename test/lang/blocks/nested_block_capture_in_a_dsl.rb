# zeo refuses to compile this program:
#
#     a nested escaping block capturing its enclosing BLOCK's own local `n`
#     isn't supported yet (zeo limitation)
#
# The shape is the standard class-macro: a block receives a value and closes
# over it in a `define_method` body. Every Rails-style declaration is written
# this way -- `has_many`, `validates`, `delegate` all build methods that
# remember an argument -- so the limitation reaches a large amount of ordinary
# ruby, and it reaches it as a compile error rather than a wrong answer.
#
# The diagnostic names the fix it wants (hoist the local to a method or the top
# level, which makes it a shared `Captured` cell); what is missing is doing that
# for the programmer, since a block-local captured by an escaping inner block
# needs exactly the same promotion an enclosing METHOD's local already gets.

klass = Class.new
klass.class_exec(5) do |n|
  define_method(:n) { n }
end
p klass.new.n

# The same shape via a method parameter already compiles -- this is the half
# that works, and the pair is what makes the limitation visible.
def build(v)
  Class.new { define_method(:v) { v } }
end
p build(7).new.v

# ...and via each, which is how a macro takes a list.
maker = Class.new
[:x, :y].each do |name|
  maker.send(:define_method, name) { name.to_s }
end
p [maker.new.x, maker.new.y]
__END__
5
7
["x", "y"]
