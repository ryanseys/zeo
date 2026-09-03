# Process::Waiter -- Process.detach's answer: a REAL thread retagged to the
# subclass, with #pid as its one own method and #value joining the reaper.
pid = Process.spawn("sleep", "0")
w = Process.detach(pid)

p w.class
p w.class.superclass
p w.pid == pid
p w.is_a?(Thread)
st = w.value
p st.class
p [st.pid == pid, st.success?]

p Process::Waiter.instance_methods(false)
p Process.constants.include?(:Waiter)

# No allocator, no new -- CRuby undefs both.
begin
  Process::Waiter.new {}
rescue TypeError, NoMethodError => e
  puts e.class
end
__END__
Process::Waiter
Thread
true
true
Process::Status
[true, true]
[:pid]
true
NoMethodError
