# A Queue hands work between two threads: the condition variable and the
# scheduler's park and wake paths run in the linked binary.
work = Queue.new
done = Queue.new
worker = Thread.new do
  while (item = work.pop)
    done << item.upcase
  end
end
%w[a b c].each { |s| work << s }
work << nil
worker.join
out = []
out << done.pop until done.empty?
p out
__END__
["A", "B", "C"]
