# One shape per file: a companion call that supplies the positional would
# immunise this (#3525).
class B
  def f(a = nil, k: :default)
    "a=#{a.inspect} k=#{k.inspect}"
  end
end
puts B.new.f(k: 1)
