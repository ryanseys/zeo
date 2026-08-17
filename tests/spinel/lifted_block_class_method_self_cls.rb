# A class method that carries the receiving class must still carry it into a
# lifted block: a sibling class-method call there used an identifier the
# proc's signature does not have.
class Base
  def self.dispatch(a)
    "#{name}:#{a}"
  end

  def self.tag
    "#{name}!"
  end

  def self.run(specs)
    out = []
    f = proc { |spec| out << dispatch(spec) }
    specs.each(&f)
    out << name
    out
  end

  def self.no_captures
    g = proc { tag }
    g.call
  end

  def self.each_tagged(xs)
    seen = []
    k = proc { |v| seen << tag }
    xs.each(&k)
    seen
  end
end

class Sub < Base
end

p Base.run(["a", "b"])
p Sub.run(["c"])
p Base.no_captures
p Sub.no_captures
p Base.each_tagged([1, 2])
p Sub.each_tagged([3])
