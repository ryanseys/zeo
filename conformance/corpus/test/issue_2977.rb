t = Thread.new { 1 }
t.join
p t.inspect.start_with?("#<Thread")
p t.inspect.include?("dead")
p t.to_s.start_with?("#<Thread")
