q = Queue.new
consumer = Thread.new do
  v = q.pop
  if v.nil?
    :got_nil
  else
    :got_value
  end
end
closer = Thread.new { q.close }
puts consumer.value
closer.join
__END__
got_nil
