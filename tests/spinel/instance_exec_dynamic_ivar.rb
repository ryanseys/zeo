# An ivar first assigned inside an instance_exec / instance_eval block
# must get a struct slot on the receiver's class. collect_ivars only
# scans method bodies, so the lift registers the slot (typed from the
# assignment). No method here references @w or @s -- the only writes are
# inside the blocks. Box has a subclass so Spinel keeps it heap.
class Box
  def initialize
    @v = 1
  end
end

class BoxPlus < Box
end

b = BoxPlus.new

# int ivar first assigned (and read) inside the block
puts b.instance_exec { @w = 7; @w + 1 }

# string ivar assigned in one block, read back in a later block
b.instance_eval { @s = "hi" }
puts b.instance_exec { @s }

# the statically-declared ivar still resolves
puts b.instance_exec { @v }
