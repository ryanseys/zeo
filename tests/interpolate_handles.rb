r, w = IO.pipe
s = "#{r} #{w}"
p s.start_with?("#<IO:")
p s.include?(" #<IO:")
pr = proc { 1 }
p "#{pr}".start_with?("#<Proc:")
f = Fiber.new { Fiber.yield 1 }
p "#{f}".start_with?("#<Fiber:")
e = [1, 2].each
p "#{e}".start_with?("#<Enumerator")
r.close
w.close
