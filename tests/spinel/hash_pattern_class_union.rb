# A hash pattern whose subject is a UNION of two user classes. The match reads
# the subject through the runtime to_h hook, which knew only Struct and Data --
# a user-defined #deconstruct_keys had no arm there, so no pattern matched and
# the else raised, naming the very class the first pattern covers (#4019). With
# ONE class the subject type is static and the method is called directly, which
# is why the union was needed to see it.
class Node
  def deconstruct_keys(keys) = to_h
end

class Lit < Node
  def initialize(v) = @v = v
  def to_h = { kind: :lit, value: @v }
end

class Col < Node
  def initialize(n) = @n = n
  def to_h = { kind: :col, value: @n }
end

def ev(node)
  case node
  in { kind: :lit, value: } then "lit #{value}"
  in { kind: :col, value: } then "col #{value}"
  else raise ArgumentError, "bad node #{node.class}"
  end
end

p ev(Lit.new(1))
p ev(Col.new("x"))

# the same shape without a shared base class, and reached through a container
class Lit2
  def initialize(v) = @v = v
  def deconstruct_keys(keys) = { kind: :lit, value: @v }
end

class Col2
  def initialize(n) = @n = n
  def deconstruct_keys(keys) = { kind: :col, value: @n }
end

def ev2(node)
  case node
  in { kind: :lit, value: } then "lit #{value}"
  in { kind: :col, value: } then "col #{value}"
  else "none"
  end
end

p ev2(Lit2.new(2))
p ev2(Col2.new("y"))
p [Lit2.new(3), Col2.new("z")].map { |n| ev2(n) }

# a Struct and a Data still answer their own members
S = Struct.new(:a)
def es(n)
  case n
  in { a: } then "s #{a}"
  else "none"
  end
end
p es(S.new(3))

D = Data.define(:b)
def ed(n)
  case n
  in { b: } then "d #{b}"
  else "none"
  end
end
p ed(D.new(b: 4))

# a class with no deconstruct_keys still falls to the else arm
class Plain; end
p ev2(Plain.new)
