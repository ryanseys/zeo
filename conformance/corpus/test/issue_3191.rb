class V
  def run(data); "#{data[:age]}/#{data.size}"; end
  def kw(name:, age: 0); "#{name}:#{age}"; end
  def mixed(x, opts); "#{x}|#{opts[:k]}"; end
end
v = V.new
p v.run(age: 5, city: "x")
p v.kw(name: "a", age: 3)
p v.mixed(1, k: 9)
p v.run({age: 7})
