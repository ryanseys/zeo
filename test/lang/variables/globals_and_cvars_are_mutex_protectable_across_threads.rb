$total = 0
class Counter
  @@n = 0
  def self.bump
    @@n = @@n + 1
  end
  def self.n
    @@n
  end
end
m = Mutex.new
t1 = Thread.new { 100.times { m.synchronize { $total += 1 } } }
t2 = Thread.new { 100.times { m.synchronize { Counter.bump } } }
t1.join
t2.join
puts $total
puts Counter.n
__END__
100
100
