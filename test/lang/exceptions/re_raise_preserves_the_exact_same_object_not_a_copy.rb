# Proves re-raise doesn't construct a fresh exception -- mutating an
# ivar (via an `attr_accessor`-generated setter) INSIDE the inner
# `rescue`, then bare `raise`-ing, must be visible to the OUTER
# `rescue` reading the same ivar back. Constructed and raised in ONE
# expression (`raise Tagged.new(...)`), not first assigned to a named
# local -- a named local assigned an `Object`-typed value inside a
# `begin`'s body (or any branching construct) hits a real, pre-existing
# codegen gap when that local's type has to widen
# to `Poly` across branches (confirmed to affect plain `if`/`else` too,
# unrelated to exception handling itself).

class Tagged < StandardError
  def initialize(msg, tag)
    super(msg)
    @tag = tag
  end
  attr_accessor :tag
end
begin
  begin
    raise Tagged.new("x", "initial")
  rescue Tagged => e
    e.send(:tag=, "mutated")
    raise
  end
rescue Tagged => e
  puts "tag: #{e.send(:tag)}"
end
__END__
tag: mutated
