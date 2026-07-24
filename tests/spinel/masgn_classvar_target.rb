# Class-variable multi-assignment targets (`@@a, @@b = 1, 2`). Each
# ClassVariableTargetNode registers and initializes its slot from the
# i-th literal of the right-hand side, mirroring a plain `@@a = 1`.
class C
  @@a, @@b = 1, 2
  @@s, @@t = "x", "y"
  def self.show
    p [@@a, @@b]
    p [@@s, @@t]
    p @@a + @@b
  end
end
C.show
