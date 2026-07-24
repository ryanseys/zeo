# `rescue => e` inside a Fiber body binds the exception local in the
# fiber body's own function. Regression: the binding was attributed to
# the enclosing scope, so `_fiber_body_N` referenced an undeclared lv_e.
fib = Fiber.new do
  begin
    raise "fib boom"
  rescue => fe
    Fiber.yield "fib: #{fe.message}"
  end
  "fib done"
end
puts fib.resume
puts fib.resume

# Distinct names bound by two rescues within one fiber body.
g = Fiber.new do
  begin; raise "one"; rescue => e1; Fiber.yield "a: #{e1.message}"; end
  begin; raise "two"; rescue => e2; Fiber.yield "b: #{e2.message}"; end
  "g done"
end
puts g.resume
puts g.resume
puts g.resume
puts "after"
