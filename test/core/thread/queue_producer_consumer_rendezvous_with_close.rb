# pop blocks the thread until a value or closure arrives; a closed empty
# queue pops nil (`thread_sync.c:1034`).

q = Queue.new
producer = Thread.new do
  q.push 1
  q.push 2
  q.close
end
consumer = Thread.new do
  total = 0
  loop do
    v = q.pop
    break if v.nil?
    total += v
  end
  total
end
producer.join
puts consumer.value
__END__
3
