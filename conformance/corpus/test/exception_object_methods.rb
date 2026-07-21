# Exception variable methods bound by `rescue => e`:
# .message returns the message string
# .class returns the class name string
# .to_s returns the message
# .inspect returns "#<ClassName: message>"
# Bare raise re-raises the original class+message rather than
# fabricating a fresh RuntimeError.

# .message
begin
  raise "boom"
rescue => e
  puts "msg: #{e.message}"
end

# .class on default raise (RuntimeError)
begin
  raise "boom2"
rescue => e
  puts "cls: #{e.class}"
end

# .class with explicit class
begin
  raise ArgumentError, "bad"
rescue => e
  puts "cls2: #{e.class}"
  puts "msg2: #{e.message}"
end

# .to_s and .inspect
begin
  raise TypeError, "wrong"
rescue => e
  puts "tos: #{e.to_s}"
  puts "ins: #{e.inspect}"
end

# Bare `raise` inside a rescue body re-raises with original class+msg.
begin
  begin
    raise ArgumentError, "inner"
  rescue => e
    raise
  end
rescue => e
  puts "re-raised cls: #{e.class}"
  puts "re-raised msg: #{e.message}"
end

# Backward-compat: `puts e` still prints the message string,
# preserving the existing test/rescue.rb shape.
begin
  raise "compat-msg"
rescue => e
  puts e
end

# Propagation when no rescue type matches (fix #3): a typed clause
# whose sp_exc_is_a check returns false must re-raise so an enclosing
# handler can catch it.
begin
  begin
    raise ArgumentError, "propagate"
  rescue TypeError => e
    puts "should not be caught here"
  end
rescue ArgumentError => e
  puts "caught propagated: #{e.class}"
  puts "msg: #{e.message}"
end

# .backtrace returns an empty str_array (#895; spinel does not
# track per-exception backtraces).
begin
  raise "trace"
rescue => e
  puts "backtrace is empty: #{e.backtrace.empty?}"
end

# .full_message returns "ClassName: message".
begin
  raise ArgumentError, "fm"
rescue => e
  puts "full: #{e.full_message}"
end

# Multi-clause rescue chain: first clause misses, second clause catches.
# Exercises the recursive sub-chain path in compile_rescue_chain.
begin
  raise TypeError, "multi"
rescue ArgumentError => e
  puts "should not match"
rescue TypeError => e
  puts "second clause: #{e.class} #{e.message}"
rescue StandardError => e
  puts "should not reach"
end

puts "done"
