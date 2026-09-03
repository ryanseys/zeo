p(->(x) { x }.to_proc.call(3))
l001 = ->(x) { x }; p l001.to_proc.call(3)
p l001.to_proc.arity
p l001.to_proc.lambda?
pr = proc { |x| x * 2 }
p pr.to_proc.call(4)
p [1,2].map(&l001.to_proc)
__END__
3
3
1
true
8
[1, 2]
