class Keystore
  def self.maker
    1
  end
  def self.go
    self.maker
  end
end
puts Keystore.go
class P157
  def self.base; "b"; end
end
class C157 < P157
  def self.go2
    self.base + "!"
  end
  def self.go3
    self.create! { |x| x * 2 }
  end
  def self.create!
    yield 21
  end
end
puts C157.go2
p C157.go3
class I157
  def val; 7; end
  def read; self.val + 1; end
end
p I157.new.read
