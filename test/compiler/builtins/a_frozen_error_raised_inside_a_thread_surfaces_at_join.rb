a = [1]
a.freeze
t = Thread.new { a[0] = 9 }
begin
  t.join
rescue FrozenError => e
  puts "at join: #{e.send(:message)}"
end
__END__
at join: can't modify frozen Array: [1]
#@ stderr
#<Thread:0xADDR compiler/builtins/a_frozen_error_raised_inside_a_thread_surfaces_at_join.rb:3 run> terminated with exception (report_on_exception is true):
compiler/builtins/a_frozen_error_raised_inside_a_thread_surfaces_at_join.rb:3:in 'block in <main>': can't modify frozen Array: [1] (FrozenError)
