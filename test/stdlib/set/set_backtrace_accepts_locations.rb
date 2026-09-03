# `Exception#set_backtrace`'s TypeError names what ruby 4 accepts: "an
# Array of String or an Array of Thread::Backtrace::Location"; zeo's
# message still offers "a single String" (an older contract). (Found by
# the 2026-08-24 probe sweep.)
begin
  RuntimeError.new.set_backtrace(42)
rescue TypeError => e
  puts e.message
end
__END__
backtrace must be an Array of String or an Array of Thread::Backtrace::Location
