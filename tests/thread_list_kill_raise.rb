# Thread.list (the live-thread registry) and Thread#kill/#raise.
Thread.report_on_exception = false

p Thread.list.size                       # 1 (just main)
p Thread.list.include?(Thread.current)   # true
# The three are HELD blocked while they are counted. A thread that has run to
# the end leaves the registry whether or not anyone joined it, so counting
# racing threads would count luck.
gate = Queue.new
threads = (1..3).map { Thread.new { gate.pop } }
sleep 0.005 until threads.all? { |t| t.status == "sleep" }
p Thread.list.size                       # 4 (main + 3 blocked)
3.times { gate << :go }
threads.each(&:join)
p Thread.list.size                       # 1 (spawns finished)
p Thread.list.include?(Thread.main)      # true

# #kill runs the ensure of a thread blocked on an empty Queue.
q = Queue.new
log = []
t = Thread.new do
  begin
    log << :started
    q.pop
    log << :unreached
  ensure
    log << :ensure_ran
  end
end
Thread.pass
t.kill
t.join
p log                                    # [:started, :ensure_ran]
p t.alive?                               # false

# #raise injects an exception the thread rescues.
q2 = Queue.new
r = Thread.new do
  begin
    q2.pop
    "no"
  rescue => e
    "caught: #{e.message}"
  end
end
Thread.pass
r.raise("boom")
p r.value                                # "caught: boom"

# #kill returns the thread; #exit / #terminate are aliases.
v = Thread.new { q.pop }
p v.kill.equal?(v)                       # true
v.join
puts "done"
