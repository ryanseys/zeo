r = Ractor.new(20, 22) do |a, b|
  a + b
end
puts r.value
__END__
42
#@ stderr
core/thread/ractor_runs_in_parallel_and_returns_its_value.rb:1: warning: Ractor API is experimental and may change in future versions of Ruby.
