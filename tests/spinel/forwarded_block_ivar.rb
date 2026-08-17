class Holder
  def store(&b); @proc = b; end
  def run; @proc.call; end
end
def forward(h, &b); h.store(&b); end
h = Holder.new
n = 0
forward(h) { n += 1 }
h.run
puts "n=#{n}"
h.run
puts "n=#{n}"

class Registry
  def initialize; @cbs = []; end
  def on(&b); @cbs << b; end
  def fire; @cbs.each { |c| c.call }; end
end
def register(r, &b); r.on(&b); end
r = Registry.new
count = 0
register(r) { count += 2 }
register(r) { count += 3 }
r.fire
p count
