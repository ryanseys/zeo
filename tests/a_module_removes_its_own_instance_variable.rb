# `instance_variables` and `remove_instance_variable` have to agree about a
# CLASS or MODULE receiver, and they did not.
#
# A class keeps its ivars in a store of their own, and only the write and the
# listing knew about it -- the removal fell through to the plain-object store,
# found nothing, and raised NameError for a name the listing had just
# reported. `Bundler::Plugin.reset!` runs the pair one line apart:
#
#   instance_variables.each { |i| remove_instance_variable(i) }
#
# so every `bundle` command died on its own reset.

module M
  module_function

  def seed
    @sources = { a: 1 }
    @commands = {}
  end

  def wipe = instance_variables.each { |i| remove_instance_variable(i) }
end

M.seed
p M.instance_variables
p M.instance_variable_defined?(:@sources)

# The removal answers what the slot held, as ruby's does.
p M.remove_instance_variable(:@sources)
p M.instance_variables
p M.instance_variable_defined?(:@sources)

# A removed name reads back as nil, not as a stale value.
p M.instance_variable_get(:@sources)

# Removing what is not there raises, and names the ivar.
begin
  M.remove_instance_variable(:@sources)
rescue NameError => e
  p [e.class, e.message]
end

# The whole loop, which is bundler's own.
M.seed
M.wipe
p M.instance_variables

# A CLASS behaves the same way, and a re-seed re-lists in assignment order.
class C
  def self.seed
    @b = 2
    @a = 1
  end
end
C.seed
p C.instance_variables
p C.remove_instance_variable(:@b)
p C.instance_variables
C.instance_variable_set(:@b, 3)
p C.instance_variables

# A frozen class refuses the removal.
class F
  def self.seed = @x = 1
end
F.seed
F.freeze
begin
  F.remove_instance_variable(:@x)
rescue => e
  p [e.class, e.message]
end
