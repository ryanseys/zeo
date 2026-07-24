m = Mutex.new
m.synchronize { puts "locked" }
puts "done"

x = m.synchronize { 21 * 2 }
puts x
puts(m.synchronize { "result" })

require "monitor"
mon = Monitor.new
mon.synchronize { puts "monitor" }

class Counter
  def initialize
    @lock = Mutex.new
    @count = 0
  end
  def bump
    @lock.synchronize { @count = @count + 1 }
  end
  def count
    @count
  end
end
c = Counter.new
c.bump
c.bump
c.bump
puts c.count
