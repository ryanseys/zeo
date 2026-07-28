# `def self.new` is an ordinary class method: `.new` dispatches to it, and its
# `super` reaches `Class#new`'s allocate-then-`initialize`. rubygems'
# `Gem::Package::TarWriter` uses the shape to offer a block form that closes.
class Writer
  def self.new(io)
    w = super
    return w unless block_given?
    begin
      yield w
    ensure
      w.close
    end
    nil
  end

  def initialize(io) = @io = io
  attr_reader :io
  def close = @io = :closed
end

p Writer.new(:handle).io
p(Writer.new(:handle) { |w| p w.io })

# A subclass inherits the wrapper, and `super` still lands on the allocator.
class Sub < Writer
  def initialize(io)
    super
    @extra = true
  end
  attr_reader :extra
end
s = Sub.new(:sub)
p [s.class, s.io, s.extra]
p(Sub.new(:sub) { |w| p w.class })

# `self.new` may answer something that isn't an instance at all.
class Maker
  def self.new(*) = :not_an_instance
end
p Maker.new
p Maker.new(1, 2)

# A block handed to a plain `.new` whose `initialize` ignores it is no error.
class Plain
  def initialize(v) = @v = v
  attr_reader :v
end
p(Plain.new(7) { raise "never called" }.v)
