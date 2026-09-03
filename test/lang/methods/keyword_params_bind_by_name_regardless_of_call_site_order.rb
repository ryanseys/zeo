class Kw
  def greet(x:, y: 10)
    x + y
  end
  def opts(**rest)
    rest.length
  end
end
k = Kw.new
puts k.greet(x: 1)
puts k.greet(x: 1, y: 2)
puts k.greet(y: 3, x: 4)
puts k.opts(a: 1, b: 2, c: 3)
__END__
11
3
7
3
