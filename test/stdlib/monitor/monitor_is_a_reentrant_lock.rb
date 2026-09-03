# `Monitor` is reentrant where `Mutex` deadlocks: the owner may enter again,
# and the lock releases only at the outermost exit.

require "monitor"
m = Monitor.new
m.synchronize do
  m.synchronize { p m.mon_owned? }
  p m.mon_locked?
end
p m.mon_locked?
p(m.synchronize { 21 * 2 })
__END__
true
true
false
42
