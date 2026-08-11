class B
  def self.who
    name
  end
  def self.make
    new
  end
  def self.tally
    @count = (@count || 0) + 1
  end
  def self.count = @count
  def self.describe_self
    "#{self} / #{self.name} / #{to_s}"
  end
end

k = Class.new(B)
def k.name = "K"

puts B.who
puts k.who
puts k.make.class == k
puts k.describe_self
B.tally
k.tally
k.tally
p [B.count, k.count]

# variable-held compile-time class still right
h = B
puts h.who
# sibling class-method call through dynamic self
class Registry
  def self.create
    build(:x)
  end
  def self.build(tag)
    "#{name}:#{tag}"
  end
end
r = Class.new(Registry)
def r.build(tag) = "override:#{tag}"
puts Registry.create
puts r.create
