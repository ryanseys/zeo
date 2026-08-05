# Two movable-set divergences. CRuby migrates an IO across a `move:` send
# (the fd travels; the source IO husks -- `$stdout` included, which is how
# this was discovered); zeo's IO handles are Arc-shared and refuse with
# `can not move IO object.` And a moved Range husks in CRuby (a Range is a
# heap object there); zeo's Range is an inline value with no shared
# identity, so the shell survives -- its heap ENDPOINTS still move.
$stderr.reopen(IO::NULL)

r = Ractor.new { Ractor.receive; :got }
f = File.open(__FILE__)
moved = begin
  r.send(f, move: true)
  true
rescue Ractor::Error => e
  p [:refused, e.message]
  r.send(:placeholder)
  false
end
p r.value
if moved
  begin
    f.fileno
  rescue Ractor::MovedError => e
    p [:io_husk, e.class]
  end
end

r2 = Ractor.new { Ractor.receive }
lo = +"a"
rg = (lo..+"z")
r2.send(rg, move: true)
p r2.value
begin
  p rg.class
rescue Ractor::MovedError => e
  p [:range_husk, e.class]
end
begin
  lo.length
rescue Ractor::MovedError => e
  p [:endpoint_poisoned, e.class]
end
