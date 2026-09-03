# Two movable-set divergences, and both follow from zeo's VALUE MODEL rather
# than from a ractor decision.
#
# 1. An IO does not migrate. CRuby moves the fd across a `move:` send and husks
#    the source IO -- `$stdout` included, which is how this was found. zeo's IO
#    handles are `Arc`-shared, so a move would hand two ractors the same
#    allocation; it refuses with `can not move IO object.` instead.
#
# 2. A moved Range does not husk. A Range is a heap object in CRuby, so the
#    shell poisons; zeo's Range is an inline value with no shared identity, so
#    there is nothing to poison and the shell survives. The last line is the
#    sharp edge of how narrow that is: the ENDPOINTS move and poison correctly,
#    and `lo.length` does raise `Ractor::MovedError` on both sides. Fixing the
#    shell means giving Range a heap identity, which is a value-representation
#    change, not a ractor one.
#
# --- ruby 4.0.6 answers ---
# :got
# [:io_husk, Ractor::MovedError]
# "a".."z"
# [:range_husk, Ractor::MovedError]
# [:endpoint_poisoned, Ractor::MovedError]

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
# The two above are DECISIONS, so this is a passing test whose golden records
# ZEO's output; the `.divergence` sidecar carries the reason and ruby's own
# answer.
#
# A third divergence this file used to record was a real BUG, and it is fixed:
# a ractor that died of an uncaught exception printed NO report, where CRuby
# prints the Thread banner. See `a_ractor_reports_its_uncaught_exception.rb`.
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
__END__
[:refused, "can not move IO object."]
:got
"a".."z"
Range
[:endpoint_poisoned, Ractor::MovedError]
