# `define_singleton_method(:literal) { ... }` desugars to a `def self.name`
# node so the installed method has a compile-time home. The node carries an
# `is_def` flag saying whether it came from the `def` KEYWORD, and the desugar
# set it `true` on the grounds that the install path reads `is_class_method`
# instead. Three other things read it, and all three were wrong:
#
#   THE BODY IS A CLOSURE. `captures`'s `DefMethod` arm walks a body only when
#   it is reachable as an escaping proc -- a real `def` opens a scope of its
#   own and cannot see an enclosing local at all. Marked as a `def`, the block
#   was never walked, so an enclosing local it reads was never promoted to a
#   shared cell: the closure fresh-declared the name and answered `nil`. Where
#   the desugar sat inside ANOTHER block, the same miss surfaced instead as
#   codegen's escaping-block refusal (active_fields, unparser, proc_to_ast).
#
#   A BARE `super` IS AN ERROR IN IT. Ruby refuses to guess the arguments for
#   a method whose body is a block (`scope_is_define_method`); zeo ran it.
#
#   ITS FRAME IS NAMED AFTER THE BLOCK, not after the method it creates --
#   `block in <class:Traced>`, the same label `define_method` gets.
#
# `is_def: false` is what its `define_method` sibling already carried.

# --- the closure, at every scope a capture can come from --------------------
class Store
  def self.build_class_method(v)
    define_singleton_method(:from_class_method) { v }
    define_method(:from_define_method) { v }
  end

  def build_instance_singleton(v)
    define_singleton_method(:from_instance_method) { v }
  end
end

Store.build_class_method(1)
p Store.from_class_method
p Store.new.from_define_method

store = Store.new
store.build_instance_singleton(2)
p store.from_instance_method

def build_at_top_level(v)
  define_singleton_method(:from_top_level) { v }
end
build_at_top_level(3)
p from_top_level

# A `def` written inside a block, whose parameter the desugar's block reads:
# the shape that hit the escaping-block refusal.
[0].each do
  def build_inside_a_block(v)
    define_singleton_method(:from_inside_a_block) { v }
  end
end
build_inside_a_block(4)
p from_inside_a_block

# Each install is its own closure over its own capture -- not one shared cell.
class Registry
  [:a, :b].each do |key|
    define_singleton_method(:"lookup_#{key}") { key }
  end
end
p Registry.lookup_a
p Registry.lookup_b

# --- `super` ----------------------------------------------------------------
class Base
  def self.describe(n) = "Base.describe(#{n})"
end

class Implicit < Base
  define_singleton_method(:describe) { |n| super }
end

begin
  Implicit.describe(1)
rescue RuntimeError => e
  puts "#{e.class}: #{e.message}"
end

# Spelling the arguments out is how ruby says to write it.
class Explicit < Base
  define_singleton_method(:describe) { |n| super(n) }
end
p Explicit.describe(2)

# --- the frame label --------------------------------------------------------
class Traced
  define_singleton_method(:label) { caller_locations(0, 1).first.label }
end
p Traced.label
__END__
1
1
2
3
4
:a
:b
RuntimeError: implicit argument passing of super from method defined by define_method() is not supported. Specify all arguments explicitly.
"Base.describe(2)"
"block in <class:Traced>"
