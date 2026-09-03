# A compute-only loop never reaches a blocking primitive, so `#kill`/`#raise`
# have no natural delivery point. The back-edge check in every native loop
# delivers them anyway -- kill runs the ensure, raise is rescuable inside
# the body.

Thread.report_on_exception = false
killed = []
t = Thread.new do
  begin
    x = 0
    loop { x += 1 }
  ensure
    killed << :ensure_ran
  end
end
Thread.pass
t.kill
t.join
p t.alive?
p killed

log = []
u = Thread.new do
  begin
    i = 0
    i += 1 while true
    "unreached"
  rescue => e
    log << e.message
  end
end
Thread.pass
u.raise("stop it")
u.join
p log
__END__
false
[:ensure_ran]
["stop it"]
