# A `class`/`module` is an expression, and its value is the body's last
# statement -- nil for an empty body, the symbol a trailing `def` answers,
# and a nested definition's own value when that is what ends the body.
x = class Solo
  def a; end
end
p x

y = class Empty
end
p y

z = module Mod
  1 + 1
end
p z

w = class Outer
  class Inner
    7
  end
end
p w

class Holder
  M = (class Inner2; 7; end)
  N = (module Inner3; end)
end
p Holder::M, Holder::N

# The value is a RECEIVER as readily as anything else -- `end.freeze` sends to
# whatever the body answered, not to the module (regexp-examples spells its
# constant tables this way).
module Wrapper
  module Frozen
    A = [1].freeze
  end.freeze
end
p Wrapper::Frozen::A.frozen?

r = class Named2
  def m; 1; end
end.name
p r

# `undef` and `alias` answer nil, unlike the `undef_method`/`alias_method`
# sends that spell the same effect (those answer the module and the new name).
class Named
  def a; end
  def b; end
  U = (undef :a)
  A = (alias c b)
end
p Named::U, Named::A
__END__
:a
nil
2
7
7
nil
true
"m"
nil
nil
