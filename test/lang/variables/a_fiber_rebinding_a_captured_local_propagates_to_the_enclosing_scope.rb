# From the reference project's own regression corpus
# (fiber_reassign_capture.rb): REBINDING (not just mutating) a captured
# name inside the fiber must write through the shared cell.

s = "old"
f = Fiber.new do
  s = "new"
  Fiber.yield
end
f.resume
puts s
__END__
new
