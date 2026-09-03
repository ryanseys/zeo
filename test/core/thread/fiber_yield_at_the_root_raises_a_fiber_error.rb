begin
  Fiber.yield(1)
rescue FiberError => e
  puts e.send(:message)
end
__END__
attempt to yield on a not resumed fiber
