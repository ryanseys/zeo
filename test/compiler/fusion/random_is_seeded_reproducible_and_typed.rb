p(Random.new(42).rand(1000) == Random.new(42).rand(1000))
p(Random.new(1).rand(1000) != Random.new(2).rand(1000))
p Random.new(5).rand(10).class
p Random.new(5).rand(2.5).class
p Random.new(5).rand.class
p((r = Random.new(9).rand(6)) >= 0 && r < 6)
p Random.new(1).bytes(8).bytesize
p Random.new(123).seed
p Random.new(3.9).seed
__END__
true
true
Integer
Float
Float
true
8
123
3
