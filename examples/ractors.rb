# Ractor -- parallel execution with a frozen-or-copy message boundary

# args cross the boundary and bind to block params
r = Ractor.new(20, 22) do |a, b|
  a + b
end
puts r.value

# message passing
worker = Ractor.new do
  msg = Ractor.receive
  msg * 10
end
worker.send(7)
puts worker.value

# shareability: immediates always, mutable values only once deeply frozen
puts Ractor.shareable?(1)
puts Ractor.shareable?("mutable")
arr = [1, 2]
puts Ractor.shareable?(arr)
Ractor.make_shareable(arr)
puts Ractor.shareable?(arr)
puts arr.frozen?

# an unshareable message is deep-copied: later mutation doesn't reach it
echo = Ractor.new do
  Ractor.receive
end
payload = [100, 200]
echo.send(payload)
payload[0] = 999
puts echo.value
