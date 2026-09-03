r = Ractor.new do
  a = Ractor.receive
  b = Ractor.receive
  c = Ractor.receive
  a * 100 + b * 10 + c
end
r.send(1)
r.send(2)
r.send(3)
puts r.value
__END__
123
#@ stderr
core/thread/ractor_messages_are_received_in_fifo_order.rb:1: warning: Ractor API is experimental and may change in future versions of Ruby.
