t = [0].map do |w|
  Thread.new do
    w.to_s
  ensure
    nil
  end
end
p t.map(&:value)

u = [1, 2].map do |w|
  Thread.new do
    begin
      w * 2
    ensure
      nil
    end
  end
end
p u.map(&:value)

v = Thread.new do
  1.to_s
ensure
  nil
end
p v.value

f = [3].map do |w|
  Fiber.new do
    Fiber.yield w
  ensure
    nil
  end
end
p f.map(&:resume)

r = [4].map do |w|
  Thread.new do
    begin
      w
    rescue StandardError
      nil
    end
  end
end
p r.map(&:value)
