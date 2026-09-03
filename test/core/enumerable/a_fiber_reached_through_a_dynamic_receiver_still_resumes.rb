# The static fast path emits `fiber_resume` directly; a fiber held in a
# collection/ivar dispatches through the runtime's Fiber table instead.

holder = [Fiber.new { Fiber.yield 1; 2 }]
p holder[0].resume
p holder[0].alive?
p holder[0].resume
p holder[0].alive?
__END__
1
true
2
false
