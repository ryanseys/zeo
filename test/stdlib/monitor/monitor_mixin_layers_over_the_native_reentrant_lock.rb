# MonitorMixin is the Ruby half of `monitor` over the native reentrant lock.

require "monitor"
class Counter
  include MonitorMixin
  def initialize
    mon_initialize
    @n = 0
  end
  def bump; synchronize { @n += 1 }; end
  def reentrant; synchronize { synchronize { mon_owned? } }; end
  attr_reader :n
end
c = Counter.new
c.bump
c.bump
p c.n
p c.reentrant
p c.mon_owned?
__END__
2
true
false
