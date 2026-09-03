# `#raise` posted before the spawned body runs its first statement.
# CRuby has no checkpoint between thread start and the body's first
# back-edge, so the delivery always happens INSIDE the body's begin
# and the rescue catches it. zeo's block prologue checkpoints before
# the first statement; the runtime skips delivery there (the
# `body_entered` rule) so the back-edge -- inside the begin --
# delivers, matching CRuby. No `Thread.pass` here on purpose: the
# widest pre-start window is the test.

Thread.report_on_exception = false
log = []
u = Thread.new do
  begin
    i = 0
    i += 1 while true
  rescue => e
    log << e.message
  end
end
u.raise("early")
u.join
p log

killed = []
t = Thread.new do
  begin
    x = 0
    loop { x += 1 }
  ensure
    killed << :ensure_ran
  end
end
t.kill
t.join
p killed
__END__
["early"]
[:ensure_ran]
