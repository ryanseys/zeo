# A ractor that dies of an uncaught exception REPORTS it on stderr as it
# terminates, and `#value` raising `Ractor::RemoteError` is a separate
# mechanism -- a program sees both.
#
# CRuby runs a ractor's body on a thread, so what it prints is that thread's
# own banner followed by the ordinary uncaught report. zeo printed nothing:
# `ractor::finish` recorded the outcome and no one reported it, so a ractor
# that died without anyone calling `#value` failed silently. The banner
# carries no origin where `Thread#inspect`'s does, because a ractor's thread
# was not created by `Thread.new` and has no call site to name.
#
# `Thread.report_on_exception` governs it, as it governs a thread's. The
# address in the banner is process-random on both sides and the harness
# normalizes it.
Warning[:experimental] = false

r = Ractor.new { raise "boom" }
begin
  r.value
rescue Ractor::RemoteError => e
  puts "value raised #{e.class}: #{e.cause.class}"
end

Thread.report_on_exception = false
quiet = Ractor.new { raise ArgumentError, "hushed" }
begin
  quiet.value
rescue Ractor::RemoteError => e
  puts "quiet raised #{e.class}: #{e.cause.message}"
end
Thread.report_on_exception = true

# One that nobody ever asks about still reports.
Ractor.new { raise NotImplementedError, "unattended" }
sleep 0.2
puts "done"
