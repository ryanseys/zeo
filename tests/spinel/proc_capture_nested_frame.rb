[1].each do |k|
  values = [2]
  f = ->(e) { values.include?(k) }
  p f.call(0)
end

[3].each do |k|
  values = [3]
  f = ->(e) { values.include?(k) }
  p f.call(0)
end

g = lambda {
  names = ["a"]
  f = ->(e) { names.include?(e) }
  p f.call("a")
  p f.call("b")
}
g.call

%w[x y].each_with_index do |s, i|
  seen = []
  add = -> { seen << "#{s}#{i}" }
  add.call
  add.call
  p seen
end
