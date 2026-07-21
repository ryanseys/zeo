p(Random.new(5).inspect.start_with?("#<Random:0x"))
r = Random.new(5)
p(r.inspect.start_with?("#<Random:0x"))
p(r.to_s.start_with?("#<Random:0x"))
p r.inspect.length
