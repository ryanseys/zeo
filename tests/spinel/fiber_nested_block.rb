# A Fiber / Enumerator.new body may contain a nested block (3.times { |i| },
# arr.each { |x| }, destructuring { |a, b| }). The body lowers to a flat C
# function and the nested block is inlined into it, so the block's parameters
# (i / x / a, b) and any locals it writes must be declared in that function.
# Previously these were missing ("lv_i undeclared") because the local
# collector stopped at nested blocks.

f1 = Fiber.new do
  3.times do |i|
    Fiber.yield i
  end
end
p f1.resume
p f1.resume
p f1.resume

f2 = Fiber.new do
  total = 0
  [1, 2, 3, 4].each do |n|
    sq = n * n
    total += sq
    Fiber.yield total
  end
end
p f2.resume
p f2.resume
p f2.resume
p f2.resume

# Enumerator.new generator with a nested block.
e = Enumerator.new do |y|
  3.times do |i|
    y << i * 100
  end
end
p e.next
p e.next
p e.next

# Two-parameter (destructuring) nested block.
f3 = Fiber.new do
  [[1, 2], [3, 4]].each do |a, b|
    Fiber.yield a + b
  end
end
p f3.resume
p f3.resume
