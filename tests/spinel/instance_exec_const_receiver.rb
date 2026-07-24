# A constant bound to an instance (`CONST = Foo.new`) works as an
# instance_exec receiver: recv_class_idx_for_rebind resolves the
# constant's registered obj_<Class> type, the same way the local and
# ivar receiver forms resolve theirs.

class Counter
  def initialize(n)
    @n = n
  end

  def bump
    @n + 1
  end
end

CTR = Counter.new(41)

puts(CTR.instance_exec { @n })      #=> 41
puts(CTR.instance_exec { bump })    #=> 42
puts "done"
