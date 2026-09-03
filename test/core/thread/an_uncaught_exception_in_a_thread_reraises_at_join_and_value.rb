# CRuby stores the exception and re-raises it in whoever joins
# (`thread.c:1195`). The no-join report_on_exception stderr warning is
# a documented skip.

bad = Thread.new { raise "thread boom" }
begin
  bad.join
rescue RuntimeError => e
  puts "joined error: #{e.send(:message)}"
end
bad2 = Thread.new { raise "thread boom2" }
begin
  bad2.value
rescue RuntimeError => e
  puts "valued error: #{e.send(:message)}"
end
__END__
joined error: thread boom
valued error: thread boom2
#@ stderr
#<Thread:0xADDR core/thread/an_uncaught_exception_in_a_thread_reraises_at_join_and_value.rb:5 run> terminated with exception (report_on_exception is true):
core/thread/an_uncaught_exception_in_a_thread_reraises_at_join_and_value.rb:5:in 'block in <main>': thread boom (RuntimeError)
#<Thread:0xADDR core/thread/an_uncaught_exception_in_a_thread_reraises_at_join_and_value.rb:11 run> terminated with exception (report_on_exception is true):
core/thread/an_uncaught_exception_in_a_thread_reraises_at_join_and_value.rb:11:in 'block in <main>': thread boom2 (RuntimeError)
