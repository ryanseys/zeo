mu = Mutex.new
r = mu.synchronize { break 5 }
puts r
puts mu.locked?
__END__
5
false
