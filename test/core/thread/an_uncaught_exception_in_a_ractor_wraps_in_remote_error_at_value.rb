# The port model relays the failure as CRuby does: a
# `Ractor::RemoteError` whose `#cause` is the original exception.

bad = Ractor.new do
  raise "ractor boom"
end
begin
  bad.value
rescue Ractor::RemoteError => e
  puts "rescued: #{e.send(:message)} / #{e.cause.class} / #{e.cause.send(:message)}"
end
__END__
rescued: thrown by remote Ractor. / RuntimeError / ractor boom
#@ stderr
core/thread/an_uncaught_exception_in_a_ractor_wraps_in_remote_error_at_value.rb:4: warning: Ractor API is experimental and may change in future versions of Ruby.
#<Thread:0xADDR run> terminated with exception (report_on_exception is true):
core/thread/an_uncaught_exception_in_a_ractor_wraps_in_remote_error_at_value.rb:5:in 'block in <main>': ractor boom (RuntimeError)
