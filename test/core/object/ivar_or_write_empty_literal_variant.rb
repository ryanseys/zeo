# An empty `{}` or `[]` carries no element to type it, so what it holds is
# decided by the reads and writes elsewhere. `@reg ||= {}` has to reach the
# same answer as `@reg = {}` would: one container, agreed on by the writes
# and the reads alike.
module Registry
  @reg ||= {}
  def self.register(extension, klass)
    extension = [extension] unless Array === extension
    extension.each { |e| @reg[e] = klass }
  end
  def self.find(path)
    @reg[path.gsub(/.*\./, "")]
  end
end
Registry.register("rb", Object)
Registry.register(["txt", "md"], String)
p Registry.find("x.rb")
p Registry.find("notes.md")
p Registry.find("x.zzz")

# The same on an instance ivar, and with the empty literal on the right of a
# `&&=` as well as a `||=`.
class Bag
  def initialize
    @items ||= {}
    @seen  ||= []
  end
  def add(k, v)
    @items[k] = v
    @seen << k
    self
  end
  def get(k) = @items[k]
  def seen = @seen
end
b = Bag.new
b.add(1, "one").add(:two, 2)
p b.get(1)
p b.get(:two)
p b.seen

# A poly-keyed write and a String-keyed read on the same slot: the pairing that
# split the variant in the first place.
module Mixed
  @m ||= {}
  def self.put(k) = @m[k] = k.to_s
  def self.get(k) = @m[k]
end
Mixed.put("s")
Mixed.put(7)
p Mixed.get("s")
p Mixed.get(7)
__END__
Object
String
nil
"one"
2
[1, :two]
"s"
"7"
