# The Enumerable desugar puts `__enum_to_a` in front of `obj.map { }` when the
# receiver's class defines #each. That rewrite is on the AST and permanent, so
# a receiver that was an object type when the desugar ran and widened to poly
# afterwards arrives at the dispatch as an Array -- and was told it has no such
# method:
#
#   undefined method '__enum_to_a' for an instance of Array (NoMethodError)
#
# Reached here because a Struct member and an unrelated class's instance method
# share a name, which is what let the parameter look like the Struct for long
# enough (#4150). The dispatch now carries the builtin arm, the way `join`
# already does for the same reason (#4071).
module Contract
  Pair = Struct.new(:registered)

  def self.spell(params)
    params.map { |x| x }
  end

  def self.pick(params)
    params.select { |x| true }
  end

  def self.walk
    [Pair.new].each do |pair|
      Contract.spell(pair.registered)
    end
  end

  class Builder
    def registered
      Pair.new
    end
  end
end

# an Array reaches the parameter the desugar typed as the Struct
p Contract.spell(Contract::Pair.new([1]).registered)
p Contract.spell([1, 2, 3])
p Contract.spell(["a", "b"])
p Contract.spell([1.5, 2.5])
p Contract.pick([1, 2])

# the Struct itself still goes through its own arm
p Contract.spell(Contract::Pair.new(9))

# and a Hash, which materializes as pairs
p Contract.spell({ a: 1 })
