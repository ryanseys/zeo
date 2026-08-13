# The walks that ask "does this body use its block?", "does it contain a
# `super`?" and "does a `begin` here need the method's return catch?" all stop
# at the same three boundaries, and each used to spell those stops out in its
# own copy of an eighty-one-arm match. `HirNode::scope_kind` answers it once.
# These are the boundaries, exercised from the outside.

# A `def` nested in a method body is a fresh scope: its `yield` is its OWN
# block's, and the enclosing method needs no block parameter for it.
class Definer
  def install
    def installed
      yield :from_installed
    end
    :installed
  end
end

d = Definer.new
p d.install
p(d.installed { |v| "inner got #{v}" })

# A `class` body is the same boundary.
def define_holder
  Class.new do
    def speak
      yield :never
    end
  end
  :defined
end

p define_holder

# A lambda catches its OWN `return`: it returns from the lambda, never from
# the enclosing method, so the method keeps running.
def lambda_return
  l = -> { return :from_lambda }
  [l.call, :method_continued]
end

p lambda_return

# ...while a `return` in a plain block returns from the METHOD, which is the
# case the `begin`/rescue catch exists for.
def block_return
  [1, 2].each { |v| return :"returned_at_#{v}" }
  :never_reached
end

p block_return

# A `begin` wrapping that block still lets the method-level return through.
def rescued_block_return
  begin
    [1].each { return :through_begin }
  rescue StandardError
    :rescued
  end
  :never_reached
end

p rescued_block_return

# A block and a lambda have no `super` target of their own, so a `super`
# inside one targets the enclosing METHOD -- neither is a stop.
class Sup
  def name; "sup"; end
end

class SubSup < Sup
  def name
    ["x"].map { super }.first + "!"
  end
end

p SubSup.new.name
