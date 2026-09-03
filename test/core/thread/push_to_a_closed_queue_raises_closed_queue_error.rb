qq = Queue.new
qq.close
puts qq.closed?
begin
  qq.push 1
rescue ClosedQueueError => e
  puts e.send(:message)
end
__END__
true
queue closed
