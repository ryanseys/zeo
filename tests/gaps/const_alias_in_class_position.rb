# A qualified constant that ALIASES a class resolves as a VALUE but not in
# CLASS position: `rescue M::ALIAS` and `is_a?(M::ALIAS)` both report the
# constant as uninitialized, though `p M::ALIAS` prints it.
module M
  class Base < StandardError
  end
  ALIAS = Base
end

p M::ALIAS
p M::ALIAS.new("x").class
p M::Base.new("x").is_a?(M::ALIAS)
begin
  raise M::Base, "boom"
rescue M::ALIAS => e
  p [:rescued, e.message]
end
