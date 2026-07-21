# Issue #895: Exception#backtrace returns an empty str_array
# (spinel doesn't track per-exception frames -- same rationale
# as the deferred Kernel#caller in #878). Returning [] keeps
# `.first` / `.length` from segfaulting on a nil receiver.
begin
  raise ArgumentError, "bad arg"
rescue => e
  puts e.class
  puts e.message
  puts e.backtrace.inspect
  puts e.backtrace.empty?
end
