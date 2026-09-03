def show2
  yield
end
show2 { |a, b| puts "a=#{a.inspect} b=#{b.inspect}" }

def show3
  yield 1
end
show3 { |a, b, c| puts "a=#{a.inspect} b=#{b.inspect} c=#{c.inspect}" }

def multi
  yield 1, 2
  yield 3
end
multi { |p, q| puts "p=#{p.inspect} q=#{q.inspect}" }

def one
  yield 10
end
one { |x, y| puts x.nil?; puts y.nil? }
__END__
a=nil b=nil
a=1 b=nil c=nil
p=1 q=2
p=3 q=nil
false
true
