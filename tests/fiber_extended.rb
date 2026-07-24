# Fiber.current has a stable identity (the root fiber at the top level)
p Fiber.current.class
p Fiber.current.equal?(Fiber.current)

# Fiber storage: Fiber[] / Fiber[]= on the current fiber, inherited at creation
Fiber[:level] = "outer"
seen = []
f = Fiber.new do
  seen << Fiber[:level]        # inherits "outer"
  Fiber[:level] = "inner"      # private write
  seen << Fiber[:level]
  seen << Fiber.current.equal?(f)
  Fiber.yield
end
f.resume
p seen
p Fiber[:level]                # root still sees "outer"

# Fiber#storage is nil until first write, then a Hash; only on the current fiber
root = Fiber.current
p root.storage                 # {level: "outer"} now
other = Fiber.new { Fiber.yield }
other.resume
begin
  other.storage                # not the current fiber -> ArgumentError
rescue ArgumentError => e
  puts e.message
end

# Fiber#kill terminates a suspended fiber and returns the fiber itself
k = Fiber.new { Fiber.yield 1; Fiber.yield 2 }
p k.resume
p k.kill.equal?(k)
p k.alive?

# Fiber#raise injects an exception at the suspended yield point
r = Fiber.new do
  begin
    Fiber.yield "one"
  rescue => e
    Fiber.yield "rescued: #{e.message}"
  end
end
p r.resume
p r.raise("boom")

