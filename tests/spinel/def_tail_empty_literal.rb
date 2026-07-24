class A
  def items
    []
  end
  def opts
    {}
  end
  def self.defaults
    {}
  end
end
puts A.new.items.length
puts A.new.opts.length
puts A.defaults.length
p A.new.items
p A.new.opts
a = A.new.items
a << 5
p a
h = A.new.opts
h["k"] = 1
p h.length
