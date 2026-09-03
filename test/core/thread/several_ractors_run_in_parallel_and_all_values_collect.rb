r1 = Ractor.new(1) { |n| n * 10 }
r2 = Ractor.new(2) { |n| n * 10 }
r3 = Ractor.new(3) { |n| n * 10 }
puts r1.value + r2.value + r3.value
__END__
60
#@ stderr
core/thread/several_ractors_run_in_parallel_and_all_values_collect.rb:1: warning: Ractor API is experimental and may change in future versions of Ruby.
