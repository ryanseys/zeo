# `raise(*exc)` -- a splat whose element count is a runtime value -- routes
# through the runtime `Kernel#raise` (optparse's `{|*exc| raise(*exc)}`),
# rather than the static 0..3-arg form.

def relay
  yield
rescue => e
  exc = [ArgumentError, "wrapped: #{e.message}"]
  raise(*exc)
end
begin
  relay { raise "orig" }
rescue ArgumentError => e
  puts e.message
end
__END__
wrapped: orig
