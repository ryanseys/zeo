# CRuby answers `<<` and `[]=` on a String/Array/Hash with a SPECIALIZED VM
# instruction (`opt_ltlt`, `opt_aset`), which pushes no control frame -- so a
# raise from inside one names only the caller. The general fallback does frame,
# and that is per RECEIVER TYPE: `opt_ltlt` covers String and Array and nothing
# else, so `IO#<<` still reports itself.
begin
  "x".freeze << "y"
rescue FrozenError => e
  p e.backtrace
end

begin
  [].freeze << 1
rescue FrozenError => e
  p e.backtrace
end

begin
  [].freeze[0] = 1
rescue FrozenError => e
  p e.backtrace
end

begin
  {}.freeze[:a] = 1
rescue FrozenError => e
  p e.backtrace
end

# A row with no specialized instruction keeps its frame.
begin
  "s".freeze[0] = "z"
rescue FrozenError => e
  p e.backtrace.first
end

begin
  "abc".freeze.concat("d")
rescue FrozenError => e
  p e.backtrace.first
end

begin
  [1, 2].freeze.sort!
rescue FrozenError => e
  p e.backtrace.first
end

# A HAND-WRITTEN `<<` is an ordinary method and frames like one.
class Sink
  def <<(_v) = raise("no")
end
begin
  Sink.new << 1
rescue RuntimeError => e
  p e.backtrace.first
end
__END__
["core/tracepoint/frozen_error_backtrace_extra_frame.rb:7:in '<main>'"]
["core/tracepoint/frozen_error_backtrace_extra_frame.rb:13:in '<main>'"]
["core/tracepoint/frozen_error_backtrace_extra_frame.rb:19:in '<main>'"]
["core/tracepoint/frozen_error_backtrace_extra_frame.rb:25:in '<main>'"]
"core/tracepoint/frozen_error_backtrace_extra_frame.rb:32:in 'String#[]='"
"core/tracepoint/frozen_error_backtrace_extra_frame.rb:38:in 'String#concat'"
"core/tracepoint/frozen_error_backtrace_extra_frame.rb:44:in 'Array#sort!'"
"core/tracepoint/frozen_error_backtrace_extra_frame.rb:51:in 'Sink#<<'"
