def a(x = nil, **kw)
  puts "x=#{x.inspect} kw=#{kw.inspect}"
end

a(k: 1)
a
a(7)
a({ h: 3 }, k: 4)

def b(x = :none, **kw)
  puts "b x=#{x.inspect} kw=#{kw.inspect}"
end

b(z: 9)
b

def c(x = nil, **kw)
  if x
    puts "c truthy #{x}"
  else
    puts "c falsy #{x.inspect}"
  end
end

c(q: 2)
c("here")
