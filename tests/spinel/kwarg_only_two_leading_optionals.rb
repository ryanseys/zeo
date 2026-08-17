# One shape per file: a companion call that supplies the positional would
# immunise this (#3525).
class C
  def self.f(a = nil, b = nil, k: :default)
    "a=#{a.inspect} b=#{b.inspect} k=#{k.inspect}"
  end
end
puts C.f(k: 1)
