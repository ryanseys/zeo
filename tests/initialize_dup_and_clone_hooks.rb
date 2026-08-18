class WithHooks
  def initialize_copy(o) = (super; @copied = :copy)
  def initialize_dup(o) = (super; @dup = :dup)
  def initialize_clone(o, freeze: nil) = (super; @clone = :clone)
  def state = instance_variables.sort
end

a = WithHooks.new
p a.dup.state
p a.clone.state
