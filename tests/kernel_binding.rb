# `Kernel#binding` at the top level: the Binding shares the frame's slots, so
# every write is visible through both sides -- and a local declared LATER in
# the scope is already in it, because a Binding names the whole frame rather
# than a snapshot of it.
x = 100
b = binding

puts "-- identity --"
p b.class
p b.frozen?
p b.receiver.equal?(self)
p b.inspect.start_with?("#<Binding:0x")
p b.to_s == b.inspect
p b.source_location.first.end_with?("kernel_binding.rb")
p b.source_location.last

puts "-- the frame's own locals --"
p b.local_variables
p b.local_variable_get(:x)
p b.local_variable_defined?(:x)
p b.local_variable_defined?(:nope)

puts "-- writes flow both ways --"
b.local_variable_set(:x, 5)
p x
x = 7
p b.local_variable_get(:x)
b.eval("x = 42")
p x
p b.eval("x + 1")

puts "-- a name the frame has no slot for is the Binding's own --"
b.local_variable_set(:added, 7)
p b.local_variable_get(:added)
b.eval("evaled = 3")
p b.local_variables
p defined?(added)

puts "-- errors --"
begin
  b.local_variable_get(:nope)
rescue NameError => e
  p [e.class, e.message.sub(/0x\h+/, "0xADDR")]
end
begin
  b.local_variable_get(1)
rescue TypeError => e
  p [e.class, e.message]
end
begin
  Binding.new
rescue NoMethodError => e
  p [e.class, e.message]
end

puts "-- declared after the capture --"
later = 9
p b.local_variable_get(:later)
