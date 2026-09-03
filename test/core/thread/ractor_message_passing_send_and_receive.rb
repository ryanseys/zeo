worker = Ractor.new do
  msg = Ractor.receive
  msg * 10
end
worker.send(7)
puts worker.value
__END__
70
#@ stderr
core/thread/ractor_message_passing_send_and_receive.rb:1: warning: Ractor API is experimental and may change in future versions of Ruby.
