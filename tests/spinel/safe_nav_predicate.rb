x = nil
p(x&.positive?)
def f(v)
  v&.positive? ? "y" : "n"
end
puts f(5)
puts f(nil)
y = 5
p(y&.positive?)
p(y&.zero?)
p(y&.abs)
s = nil
p(s&.empty?)
