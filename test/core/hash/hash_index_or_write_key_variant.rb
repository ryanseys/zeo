# h[k] ||= <container> picks the hash variant from the key, param keys included
def build(k)
  h = {}
  h[k] ||= []
  h[k] << 1
  h
end
p build(0)
p build("a")
p build(:s)

def lit; h = {}; h[0] ||= []; h[0] << 1; h; end
p lit
def bh(k); h = {}; h[k] ||= {}; h[k]["x"] = 1; h; end
p bh(0)
def sc(k); h = {}; h[k] ||= 0; h[k] += 1; h; end
p sc(0)

class K
  def initialize; @h = {}; end
  def add(k); @h[k] ||= []; @h[k] << 1; self; end
  def to_h; @h; end
end
p K.new.add(0).to_h
__END__
{0 => [1]}
{"a" => [1]}
{s: [1]}
{0 => [1]}
{0 => {"x" => 1}}
{0 => 1}
{0 => [1]}
