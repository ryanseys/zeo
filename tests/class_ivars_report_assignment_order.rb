# `Class#instance_variables` reports names in FIRST-ASSIGNMENT order.
#
# The intern table cannot supply that order -- a name is interned the first
# time any emitted site MENTIONS it, which for a read-only `@x` happens
# before, or instead of, a write -- so a sorted answer stood in for it and
# a class assigning `@b` before `@a` reported `[:@a, :@b]`. Each slot now
# stamps its own first write.

class Ord
  @b = 1
  @a = 2
  @m = 3
end
p Ord.instance_variables

module ModOrd
  @z = 1
  @y = 2
end
p ModOrd.instance_variables

# The INSTANCE side already reports assignment order, and is here so a
# regression names which half broke.
class Inst
  def initialize
    @b = 1
    @a = 2
  end
end
p Inst.new.instance_variables
