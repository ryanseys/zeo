# A fiber's captured locals must survive a GC that fires while the
# fiber is parked. The capture struct holds the only reference to
# these objects; without a scan hook on it the collector frees them
# and a later resume reads freed memory (use-after-free). Forcing
# several collections via GC.start while multiple fibers are parked
# is what surfaces the corruption.
class Holder
  attr_reader :data
  def initialize(label)
    @data = "holder_payload_" + label
  end
end

def churn(n)
  i = 0
  s = ""
  while i < n
    s = s + i.to_s + ","
    i += 1
  end
  s.length
end

results = []

f1 = Fiber.new do
  h1 = Holder.new("one")
  Fiber.yield
  results.push(h1.data)
end

f2 = Fiber.new do
  h2 = Holder.new("two")
  Fiber.yield
  results.push(h2.data)
end

f3 = Fiber.new do
  h3 = Holder.new("three")
  Fiber.yield
  results.push(h3.data)
end

f1.resume
f2.resume
f3.resume

n = 0
while n < 20
  churn(200)
  GC.start
  n += 1
end

f3.resume
f1.resume
f2.resume

puts results.length
i = 0
while i < results.length
  puts results[i]
  i += 1
end
