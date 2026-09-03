obj = Object.new
def obj.run
  a = yield 1
  b = yield 2
  [a, b]
end
p(obj.run { |n| next n * 10 })
__END__
[10, 20]
