class Dispatcher
  def greet; "hi"; end
end
d = Dispatcher.new
p d.send(:send, :greet)
p d.public_send(:public_send, :greet)
p d.send(:__send__, :greet)
p d.send(:greet)
