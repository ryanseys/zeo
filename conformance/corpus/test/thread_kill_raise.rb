# Thread#kill terminates a thread, running its ensure blocks; Thread#raise
# injects an exception caught by the thread's rescue. A thread is blocked on a
# Queue so the delivery point is deterministic (Thread.pass runs it to the
# blocking #pop). CRuby's Thread.pass is only a hint, so this asserts the
# documented semantics rather than a per-run interleaving.
Thread.report_on_exception = false

# #kill runs the ensure of a blocked thread
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
p log                  # [:started, :ensure_ran]
p t.alive?             # false

# #raise injects an exception the thread rescues
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
p r.value              # "caught: boom"

# killing a never-run thread just marks it dead (no body, no ensure)
u = Thread.new { q.pop }
u.kill
u.join
p u.alive?             # false

# #kill returns the thread; #exit / #terminate are aliases
v = Thread.new { q.pop }
p v.kill.equal?(v)     # true
v.join
