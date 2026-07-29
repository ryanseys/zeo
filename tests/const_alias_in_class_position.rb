# A constant that ALIASES a class or module (`ALIAS = Base`) reads as a value
# everywhere -- but the interesting positions are the ones that usually hold a
# literal class NAME: `rescue`, `is_a?`, and `raise`. Ruby evaluates the
# constant in all three, so the alias works there, and an undefined constant
# raises only once the expression is actually reached.
module M
  module Tag; end
  class Base < StandardError
    include Tag
  end
  ALIAS = Base
  TAGALIAS = Tag
end

p M::ALIAS
p M::ALIAS.new("x").class
p M::Base.new("x").is_a?(M::ALIAS)
p M::Base.new("x").is_a?(M::TAGALIAS)
p 1.is_a?(M::TAGALIAS)

begin
  raise M::Base, "boom"
rescue M::ALIAS => e
  p [:rescued, e.message]
end

begin
  raise M::Base, "tagged"
rescue M::TAGALIAS => e
  p [:by_module_alias, e.message]
end

# `raise` through the alias itself.
begin
  raise M::ALIAS, "through the alias"
rescue M::Base => e
  p [e.class.to_s, e.message]
end

# --- unqualified, which is the same rule one scope out ----------------------
Plain = M::Base
p M::Base.new("y").is_a?(Plain)
begin
  raise Plain, "plain"
rescue Plain => e
  p [:plain, e.message]
end

# --- an undefined constant raises only where Ruby reaches it -----------------
begin
  1
rescue NoSuchErrorClass => e
  p :never
end
p :the_clause_that_never_fired_still_compiled

begin
  begin
    raise "x"
  rescue AlsoMissing
    p :no
  end
rescue NameError => e
  p e.message
end
