# backtrace carries the real stamped frames; full_message renders the
# uncaught-report shape from them; inspect renders "#<Class: msg>" (or
# the bare class name when the message is empty). Expected output is
# verbatim ruby 4.0.6 (the harness compiles as `-e`, same as the
# oracle's own `-e` labeling).

begin
  raise ArgumentError, "bad"
rescue => e
  p e.backtrace
  p e.inspect
  puts e.full_message
end
p StandardError.new("").inspect
__END__
["core/tracepoint/exception_backtrace_full_message_and_inspect.rb:8:in '<main>'"]
"#<ArgumentError: bad>"
core/tracepoint/exception_backtrace_full_message_and_inspect.rb:8:in '<main>': bad (ArgumentError)
"StandardError"
