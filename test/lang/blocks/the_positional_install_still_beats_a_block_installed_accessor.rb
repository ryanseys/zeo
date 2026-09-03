# ...and the pass's own job is untouched: a block-installed accessor still
# loses to the `def` written below it, and so does a macro a class method
# expands. A fix that made `dyn_defs` refuse more than it should would show
# up here and nowhere else.

class Blocked
  [:y].each { |a| attr_writer(a) }
  def y=(v)
    @y = [v, v]
  end
  attr_reader :y
end
class Macro
  def self.make(n) = attr_writer(n)
  make :z
  def z=(v)
    @z = [v, v]
  end
  attr_reader :z
end
b = Blocked.new
b.y = 1
p b.y
m = Macro.new
m.z = 2
p m.z
__END__
[1, 1]
[2, 2]
