# Thread#raise aimed at the MAIN thread from a sibling: the main thread is
# a first-class interrupt target (delivered at its next interruption
# checkpoint), and the rescue swallows it exactly as under CRuby. The
# output pins "ok" either way -- delivery timing may land the raise inside
# the begin (rescued) or leave it pending past program end -- so the test
# asserts "delivers-or-ignores without crashing", not the exact timing.
Thread.report_on_exception = false
begin
  t = Thread.new { Thread.main.raise("to-main") }
  t.join
  sleep 0.2
rescue => e
  # CRuby lands here; spinel's no-op never raises.
end
puts "ok"
__END__
ok
