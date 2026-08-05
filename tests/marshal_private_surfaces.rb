# The census-visible marshal privates: Marshal's module_function dump,
# Complex/Rational marshal_dump hooks, Random's full state family.
p Marshal.private_instance_methods(false)
p Marshal.singleton_methods(false).sort

p Complex(1, 2).send(:marshal_dump)
p Rational(3, 4).send(:marshal_dump)
p Complex(1.5, 0r).send(:marshal_dump)
p Complex.private_instance_methods(false).include?(:marshal_dump)
p Rational.private_instance_methods(false).include?(:marshal_dump)

# Random: state exposes the raw MT position; a dump resumes the stream.
r = Random.new(42)
p r.send(:left)
p r.send(:state) % (2**32)
r.rand(100)
p r.send(:left)
d = r.send(:marshal_dump)
p [d.class, d.size, d[1], d[2]]
r2 = Random.new(0)
r2.send(:marshal_load, d)
p r2.rand(100) == r.rand(100)
p r2.rand == r.rand
p Marshal.load(Marshal.dump(Random.new(7))).rand(1000) == Random.new(7).rand(1000)
begin
  Random.new(1).send(:marshal_load, [1, 0])
rescue ArgumentError => e
  puts e.message
end
p [Random.send(:state).class, Random.send(:left).class]
p Random.private_instance_methods(false).sort
