# A failing thread surfaces its error at its OWN join; the healthy worker
# completes normally, unaffected.

class Plain
end
worker = Thread.new { 21 * 2 }
bad = Thread.new { Plain.new.send(:missing_in_thread) }
begin
  bad.join
rescue NoMethodError => e
  puts "joined the failure"
end
puts worker.value
__END__
joined the failure
42
#@ stderr
#<Thread:0xADDR core/process/one_threads_bad_dispatch_no_longer_kills_the_other_threads.rb:7 run> terminated with exception (report_on_exception is true):
core/process/one_threads_bad_dispatch_no_longer_kills_the_other_threads.rb:7:in 'block in <main>': undefined method 'missing_in_thread' for an instance of Plain (NoMethodError)
