t = Thread.new { 1 }
t.join
p t.inspect.include?(__FILE__)
p t.to_s.include?(__FILE__)
p t.inspect.start_with?("#<Thread:0x")
p t.inspect.end_with?("dead>")
