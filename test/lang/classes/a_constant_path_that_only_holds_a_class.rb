# `M::ALIAS` where `ALIAS` is a constant HOLDING a class, not a class of its
# own, is the scope operator's question. CLIF handed the whole string to the
# bare-name cref walk, which looked for a constant literally called
# "M::ALIAS" and raised `uninitialized constant M::ALIAS` -- but only in the
# positions that lower through the class-NAME path (`New`), so `p M::ALIAS`
# answered and `M::ALIAS.new` did not.

module M
  module Tag; end
  class Base < StandardError
    include Tag
    def initialize(m) = super("base: #{m}")
  end
  ALIAS = Base
  TAGALIAS = Tag
  Made = Struct.new(:a, :b)
  MADEALIAS = Made
end

TOP = M::Base

p M::ALIAS
p M::ALIAS.new("x").message
p M::ALIAS.new("x").class
p TOP.new("y").message
p M::Base.new("x").is_a?(M::TAGALIAS)

# A runtime-minted class reached through an alias, with and without a block.
p M::MADEALIAS.new(1, 2).to_a
p M::MADEALIAS.members

# Through a deeper path, and through an alias OF an alias.
module Outer
  module Inner
    Leaf = Class.new { def hi = :hi }
  end
  LEAFALIAS = Inner::Leaf
end
p Outer::Inner::Leaf.new.hi
p Outer::LEAFALIAS.new.hi

# `rescue` and `raise` name it too.
begin
  raise M::ALIAS, "boom"
rescue M::ALIAS => e
  p [e.class, e.message]
end

# A path whose LEAF is genuinely missing still says so.
begin
  M::Nope.new
rescue NameError => e
  p e.message
end
__END__
M::Base
"base: x"
M::Base
"base: y"
true
[1, 2]
[:a, :b]
:hi
:hi
[M::Base, "base: boom"]
"uninitialized constant M::Nope"
