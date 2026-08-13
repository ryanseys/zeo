# Two movable-set divergences. CRuby migrates an IO across a `move:` send
# (the fd travels; the source IO husks -- `$stdout` included, which is how
# this was discovered); zeo's IO handles are Arc-shared and refuse with
# `can not move IO object.` And a moved Range husks in CRuby (a Range is a
# heap object there); zeo's Range is an inline value with no shared
# identity, so the shell survives -- its heap ENDPOINTS still move.
#
# The Range half is the narrower of the two, and the last line below is why:
# `lo.length` DOES raise `Ractor::MovedError` in zeo. The endpoints move and
# poison correctly; only the Range shell wrapping them cannot husk, because
# there is no shared allocation to poison. Fixing it means giving Range a heap
# identity, which is a value-representation change, not a ractor one.
#
# ALSO DIVERGENT, not asserted here (a golden cannot hold it -- the report
# carries a heap address, and `IO#reopen` onto a file is broken besides, see
# `an_io_reopen_onto_a_file_fails.rb`): a ractor that dies of an uncaught
# exception prints NO report on stderr, where CRuby prints the same
# `#<Thread:0x...> terminated with exception (report_on_exception is true):`
# banner a Thread gets. zeo prints the THREAD form correctly, so this is a
# missing call on the ractor's own termination path -- `zeo_rt::ractor::finish`
# records the outcome and nothing reports it. `#value` still raises
# `Ractor::RemoteError` correctly, so the failure is silent rather than lost.
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
