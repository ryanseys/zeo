q = Queue.new
p1 = Thread.new { 10.times { q.push 1 } }
p2 = Thread.new { 10.times { q.push 1 } }
consumer = Thread.new do
  total = 0
  loop do
    v = q.pop
    break if v.nil?
    total += v
  end
  total
end
p1.join
p2.join
q.close
puts consumer.value
__END__
20
