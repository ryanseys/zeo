# A class built at runtime runs its body as a block, and its directives are
# rewritten into the self-sends that block serves (`lower::defs::
# runtime_directive_spelling`). `tests/runtime_class_body_directives.rb` shows
# that working for `Class.new do ... end`.
#
# Giving the class a runtime SUPERCLASS is what breaks it, and the two spellings
# break differently. Everything here lowers and runs -- these are silent wrong
# answers, not compile errors, which is why they are pinned.
#
#   `Class.new(Base) do ... end`   -- `prepend` lands in `ancestors` but does
#                                     not take over dispatch
#   `class Sub < <expr>`           -- the same, PLUS `private` does not retag,
#                                     `private_class_method` does not retag,
#                                     `class << self; undef; end` does not
#                                     retire, and a constant (or a nested
#                                     class's name) never lands on the class
#
# And underneath both: a runtime-built class does not inherit its superclass's
# CLASS methods at all. `Class.new(Base).retired` is a NoMethodError, which is
# also why an inherited name cannot be found to `undef`.
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
