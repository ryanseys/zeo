mu = Mutex.new
begin
  mu.unlock
rescue ThreadError => e
  puts e.send(:message)
end
mu.lock
puts mu.locked?
puts mu.owned?
begin
  mu.lock
rescue ThreadError => e
  puts e.send(:message)
end
mu.unlock
puts mu.locked?
__END__
Attempt to unlock a mutex which is not locked
true
true
deadlock; recursive locking
false
