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
__END__
x=nil kw={k: 1}
x=nil kw={}
x=7 kw={}
x={h: 3} kw={k: 4}
b x=:none kw={z: 9}
b x=:none kw={}
c falsy nil
c truthy here
