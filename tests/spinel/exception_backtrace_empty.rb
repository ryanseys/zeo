# Exception#backtrace carries the real stamped frames (frame tracking):
# non-empty after a raise, innermost line first. The path prefix is the
# compile-time input path, so assertions stay path-portable.
begin
  raise ArgumentError, "bad arg"
rescue => e
  puts e.class
  puts e.message
  puts e.backtrace.empty?
  puts e.backtrace.first.end_with?(":5:in '<main>'")
end
