class Item
  attr_reader :kind
  def initialize(kind) = @kind = kind

  def parse(param)
    case kind
    when :int then Integer(param, 10)
    when :str then param
    end
  end
end

def parse(item, param)
  if item.kind == :bool
    param = nil
    return true
  end
  item.parse(param)
end

values = [parse(Item.new(:bool), ""), parse(Item.new(:int), "1"), parse(Item.new(:str), "x")]
raise "FAIL" unless values == [true, 1, "x"]
puts "ok"

def conv(x)
  Integer(x, 16)
end

p conv("ff")
p(begin; conv(nil); rescue ArgumentError => e; e.class; end)
p(begin; conv(3); rescue ArgumentError => e; e.class; end)
