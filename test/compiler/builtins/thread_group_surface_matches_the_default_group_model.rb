# The always-on `ThreadGroup` surface the timeout gem needs:
# `Thread#group` answers the shared `ThreadGroup::Default`, which is
# never enclosed, accepts `#add`, and type-checks its argument.

g = Thread.current.group
p g.equal?(ThreadGroup::Default)
p g.enclosed?
t = Thread.new { 1 }
p ThreadGroup::Default.add(t).equal?(g)
t.join
begin
  g.add(42)
rescue TypeError => e
  puts e.message
end
p Thread.handle_interrupt(Exception => :never) { :ran }
__END__
true
false
true
wrong argument type Integer (expected VM/thread)
:ran
