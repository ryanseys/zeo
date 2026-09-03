# A class built at runtime runs its body as a block, and its directives are
# rewritten into the self-sends that block serves (`lower::defs::
# runtime_directive_spelling`). `tests/runtime_class_body_directives.rb` shows
# that working for `Class.new do ... end`.
#
# Giving the class a runtime SUPERCLASS used to break five separate things, in
# both spellings. None of them was a compile error -- every one lowered, ran,
# and answered wrongly -- which is why this file exists.
#
#   CLASS METHODS were not inherited at all. `class_methods` is a FLAT table,
#   on the stated ground that compile-time materialization already copies every
#   inherited `def self.x` onto each subclass. A class born at runtime was never
#   in that pass, so its table was empty and nothing walked past it:
#   `Class.new(Base).retired` raised however ordinary `retired` was. Dispatch
#   now falls back to the ancestor walk, the class-method twin of `lookup_mro`.
#
#   PREPEND landed in `ancestors` -- correctly first -- and never took over
#   dispatch. A module's bodies are in neither the flat table nor `own_impls`:
#   `emit_user_module_bridges` registers them as VALUE METHODS on the module's
#   own id, which `super`'s walk probes and `walk_runtime_class` did not. It
#   reached the module's position, found empty tables, and carried on.
#
#   `private :m` NAMING A METHOD THE SAME BODY DEFINES is retagged onto the
#   `def` at lowering time, so no directive node survives for the rewrite. On
#   the static path the mark rides the `Scope` into the dispatch row; here the
#   `def` becomes a runtime definition that carries no visibility, so it has to
#   be re-spoken as a send. Same for `private_class_method`, on the singleton.
#
#   `class << self; undef x; end` tombstoned `x` under the SINGLETON's id, and
#   nothing on the class-method path looks at a singleton id. The retirement
#   now lands in the owner's class-method space (`OverlayEntry::class_undefs`).
#
#   A CONSTANT resolves its owner from the enclosing cref at compile time, and
#   a runtime class body is a block whose cref is whatever encloses it --
#   `Object` at the top level. `OPEN = :visible` wrote `Object::OPEN`. It is
#   re-pointed at the block's `self`, which is the class being built; a nested
#   `class Inner` needs the same, and gets it in `runtime_nested_class`.
#
#   The READ needs re-pointing too, and this file only checked `Sub::OPEN` from
#   OUTSIDE at first. A bare constant lowers to a name codegen resolves against
#   the lexically-enclosing class, so once the write moved to the built class
#   the two looked in different places: `def reads_open = OPEN` raised
#   `uninitialized constant`. `rescope_body_constants` moves it, and the two
#   positions need different scopes -- in the body `self` IS the class, while
#   in a `def` the class has to be named through the constant holding it,
#   because that constant is not assigned until the whole body has run.
#
# Only the `class` KEYWORD spelling opens a cref. `Class.new do NAME = v end`
# is a plain block whose cref is the enclosing one, so ruby writes the constant
# on `Object` there -- checked at the bottom, since re-pointing it would be
# just as wrong in the other direction.
#
# The three compiler panics that led here (danger, gitlab-labkit,
# activeadmin_settings_cached) are fixed and separate: those reached codegen
# through directives the rewrite table had no row for at all.

class Base
  def size = 1
  def self.retired = :retired
end

module Front
  def size = "front(#{super})"
end

def parent_of(x) = x ? Base : Object

# --- `Class.new(Super) do ... end` -------------------------------------------
Made = Class.new(Base) do
  def size = 2
  prepend Front
end

p Made.ancestors.first
p Made.new.size
p Made.retired

# --- `class Sub < <expr>` ----------------------------------------------------
class Sub < parent_of(true)
  OPEN = :visible

  def size = 2
  def wide = :wide
  def self.internal = :internal
  def self.own_retired = :own_retired
  def reads_open = OPEN
  def self.reads_open = OPEN
  def builds_inner = Inner.new.a
  SEEN_IN_BODY = OPEN

  prepend Front
  private :wide
  private_class_method :internal

  class << self
    undef own_retired
  end

  class Inner
    def a = :a
  end
end

p Sub.new.size
p [Sub.new.respond_to?(:wide), Sub.new.respond_to?(:wide, true)]
p Sub.singleton_class.private_method_defined?(:internal)
p Sub.respond_to?(:own_retired)
p Sub::OPEN
p Sub::Inner.new.a
p Sub.retired
p Sub.new.reads_open
p Sub.reads_open
p Sub.new.builds_inner
p Sub::SEEN_IN_BODY
p Object.const_defined?(:OPEN, false)

# --- `Class.new do ... end` opens NO cref -------------------------------------
Blocky = Class.new do
  BLOCK_LEVEL = :block_level
  def reads_it = BLOCK_LEVEL
end
p Blocky.new.reads_it
p Object.const_defined?(:BLOCK_LEVEL, false)
p Blocky.const_defined?(:BLOCK_LEVEL, false)
__END__
Front
"front(2)"
:retired
"front(2)"
[false, true]
true
false
:visible
:a
:retired
:visible
:visible
:a
:visible
false
:block_level
true
false
