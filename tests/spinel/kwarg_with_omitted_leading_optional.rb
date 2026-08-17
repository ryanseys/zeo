# A keyword argument supplied while a leading optional positional is omitted:
# the keyword hash must NOT also land in the positional slot (#3525).
class A
  def self.f(a = nil, k: :default)
    "a=#{a.inspect} k=#{k.inspect}"
  end
end
puts A.f(k: 1)
puts A.f(9, k: 1)
puts A.f
puts A.f(9)

# instance method, same shape
class B
  def f(a = nil, k: :default)
    "a=#{a.inspect} k=#{k.inspect}"
  end
end
puts B.new.f(k: 1)
puts B.new.f(9, k: 1)

# two leading optionals
class C
  def self.f(a = nil, b = nil, k: :default)
    "a=#{a.inspect} b=#{b.inspect} k=#{k.inspect}"
  end
end
puts C.f(k: 1)
puts C.f(7, k: 1)
puts C.f(7, 8, k: 1)

# a trailing required after the optional, with a keyword
class D
  def self.f(a = nil, r, k: :default)
    "a=#{a.inspect} r=#{r.inspect} k=#{k.inspect}"
  end
end
puts D.f(5, k: 1)
puts D.f(4, 5, k: 1)
