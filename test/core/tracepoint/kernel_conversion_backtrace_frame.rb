# `Kernel#Integer` and its family are ordinary cfuncs in CRuby, so they push a
# control frame and a raise from inside one names them. zeo's codegen calls
# them through a fast path that never reaches the dispatch boundary where a
# builtin's frame is otherwise pushed, so the frame is pushed in the row.
begin
  Integer("zz")
rescue ArgumentError => e
  p e.backtrace
end

begin
  Hash([[1, 2]])
rescue TypeError => e
  p e.backtrace
end

begin
  Float("zz")
rescue ArgumentError => e
  p e.backtrace.first
end

begin
  Rational("x")
rescue ArgumentError => e
  p e.backtrace.first
end

begin
  Integer(nil)
rescue TypeError => e
  p e.backtrace.first
end

# The DYNAMIC route reaches the dispatch boundary as well as the row, and must
# still show ONE frame -- `synthetic_c_frame` drops an exact repeat.
begin
  send(:Integer, "zz")
rescue ArgumentError => e
  p e.backtrace
end

begin
  method(:Integer).call("zz")
rescue ArgumentError => e
  p e.backtrace.first
end

def wrapped(s) = Integer(s)
begin
  wrapped("zz")
rescue ArgumentError => e
  p e.backtrace.first(2)
end

# `exception: false` answers nil rather than raising, so nothing is framed.
p [Integer("zz", exception: false), Float("zz", exception: false)]
__END__
["core/tracepoint/kernel_conversion_backtrace_frame.rb:6:in 'Kernel#Integer'", "core/tracepoint/kernel_conversion_backtrace_frame.rb:6:in '<main>'"]
["core/tracepoint/kernel_conversion_backtrace_frame.rb:12:in 'Kernel#Hash'", "core/tracepoint/kernel_conversion_backtrace_frame.rb:12:in '<main>'"]
"core/tracepoint/kernel_conversion_backtrace_frame.rb:18:in 'Kernel#Float'"
"core/tracepoint/kernel_conversion_backtrace_frame.rb:24:in 'Kernel#Rational'"
"core/tracepoint/kernel_conversion_backtrace_frame.rb:30:in 'Kernel#Integer'"
["core/tracepoint/kernel_conversion_backtrace_frame.rb:38:in 'Kernel#Integer'", "core/tracepoint/kernel_conversion_backtrace_frame.rb:38:in '<main>'"]
"core/tracepoint/kernel_conversion_backtrace_frame.rb:44:in 'Kernel#Integer'"
["core/tracepoint/kernel_conversion_backtrace_frame.rb:49:in 'Kernel#Integer'", "core/tracepoint/kernel_conversion_backtrace_frame.rb:49:in 'Object#wrapped'"]
[nil, nil]
