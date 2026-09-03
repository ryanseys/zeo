p rand(5...5)
p rand(5..3)
p rand(-3) >= 0
srand(3); v = rand(1..1000); p (1..1000).cover?(v)
p Random.new(1) == Random.new(1)
p Random.new(1) == Random.new(2)
r = Random.new(7); p r == r
p Random.new_seed.class
def t; yield; rescue => e; e.class; end
p t { rand(1..) }
p t { rand(..5) }
__END__
nil
nil
true
true
true
false
true
Integer
Errno::EDOM
Errno::EDOM
