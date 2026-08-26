# The Proc `Method#to_proc` builds forwards its block to the wrapped method.
#
# It used to drop it, so a block-taking method reached through `to_proc` ran
# block-less: `h.method(:each).to_proc.call { }` iterated zero times and
# answered an Enumerator where ruby answers the receiver. `Method#call` with a
# block was always right -- only the proc `to_proc` returned had no block
# channel, which also took `&method(:x)`, `UnboundMethod#bind(o).to_proc` and
# `#curry` down with it.
#
# The rows below are the whole family: the two that always worked are kept as
# controls, so a regression names which half broke.

def takes_block
  block_given? ? yield : :no_block
end

class Target
  def takes_block
    block_given? ? yield : :no_block
  end
end

# The control: a Method called directly has always carried its block.
puts method(:takes_block).call { :ran }

# The fix: through to_proc, in each of the ways a proc can be called.
pr = method(:takes_block).to_proc
puts pr.call { :ran }
puts pr.() { :ran }
puts pr.call(&-> { :ran })

# `&method(:x)` is the common idiom, and it goes through to_proc.
def relay(&blk) = blk.call { :ran }
puts relay(&method(:takes_block))

# An UnboundMethod bound to a receiver builds its proc the same way.
puts Target.instance_method(:takes_block).bind(Target.new).to_proc.call { :ran }

# curry's final application is the step that reaches the target, so the block
# rides that one.
puts method(:takes_block).to_proc.curry.call { :ran }

# Two more controls, from the neighbouring constructors that never broke.
puts :takes_block.to_proc.call(Target.new) { :ran }
puts(proc { |&b| takes_block(&b) }.call { :ran })

# The receiver comes back, not an Enumerator -- the tell that the block ran.
h = {"k" => 1}
seen = []
puts h.method(:each).to_proc.call { |k, v| seen << [k, v] }.class
p seen
