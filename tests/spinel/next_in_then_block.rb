def pick(n)
  n.then do |v|
    next [] if v.zero?
    next [v] if v < 10
    [v, v * 2]
  end
end

p pick(0)
p pick(3)
p pick(20)

p(
  [1, 2].flat_map do |v|
    v.then do |x|
      next [] if x == 1
      [x]
    end
  end
)

def tally(n)
  seen = []
  n.tap do |v|
    next if v.negative?
    seen << v
  end
  seen
end

p tally(-1)
p tally(7)

h = 0.then do |v|
  next({}) if v.zero?
  { "k" => v }
end
p h

s = "ab".then do |t|
  next "-" if t.empty?
  t.upcase
end
p s
