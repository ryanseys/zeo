# Thread.list (the live-thread registry) and Thread#kill/#raise. At the single
# scheduler worker a spawned thread does not run until the current one yields,
# so the counts and delivery points here are deterministic.
Thread.report_on_exception = false

p Thread.list.size                       # 1 (just main)
p Thread.list.include?(Thread.current)   # true
threads = (1..3).map { Thread.new { 1 } }
p Thread.list.size                       # 4 (main + 3 runnable)
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
