# Thread.list (main plus live spawns, joined ones pruned) and
# Thread.list.include? by identity; Thread#kill unwinding a blocked thread
# through its ensure; Thread#raise injecting a rescuable exception; #kill
# returning the thread; #exit/#terminate aliases.

Thread.report_on_exception = false
p Thread.list.size
p Thread.list.include?(Thread.current)
# The three are held blocked while they are counted: a thread that
# has already run to the end leaves `Thread.list` whether or not
# anyone joined it, so counting racing threads counts luck.
gate = Queue.new
ts = (1..3).map { Thread.new { gate.pop } }
sleep 0.005 until ts.all? { |t| t.status == "sleep" }
p Thread.list.size
3.times { gate << :go }
ts.each(&:join)
p Thread.list.size

q = Queue.new
ready = Queue.new
log = []
t = Thread.new do
  begin
    log << :started
    ready << :ok
    q.pop
    log << :unreached
  ensure
    log << :ensure_ran
  end
end
ready.pop
t.kill
t.join
p log
p t.alive?

q2 = Queue.new
r = Thread.new do
  begin
    ready << :ok
    q2.pop
    "no"
  rescue => e
    "caught: #{e.message}"
  end
end
ready.pop
r.raise("boom")
p r.value

v = Thread.new { q.pop }
p v.kill.equal?(v)
v.join
__END__
1
true
4
1
[:started, :ensure_ran]
false
"caught: boom"
true
