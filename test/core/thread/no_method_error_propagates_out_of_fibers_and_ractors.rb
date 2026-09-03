class Bare; end
f = Fiber.new { Bare.new.send(:nope) }
begin
  f.resume
rescue NoMethodError
  puts "rescued in resumer"
end
puts f.alive?
r = Ractor.new { Bare.new.send(:nope) }
begin
  r.value
rescue Ractor::RemoteError => e
  puts "rescued at value: #{e.cause.class}"
end
__END__
rescued in resumer
false
rescued at value: NoMethodError
#@ stderr
core/thread/no_method_error_propagates_out_of_fibers_and_ractors.rb:9: warning: Ractor API is experimental and may change in future versions of Ruby.
#<Thread:0xADDR run> terminated with exception (report_on_exception is true):
core/thread/no_method_error_propagates_out_of_fibers_and_ractors.rb:9:in 'block in <main>': undefined method 'nope' for an instance of Bare (NoMethodError)
