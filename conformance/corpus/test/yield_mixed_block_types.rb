# One yield method called with blocks of different value types: each call
# site carries its own block's type (a class method used to funnel every
# site through the first site's C type and segfault).
class B
  def self.tx
    yield
  end
  def itx
    yield
  end
end

puts(B.tx { "in" })
n = B.tx { 41 + 1 }
p n
B.tx { puts "side" }

puts(B.new.itx { "istr" })
m = B.new.itx { 7 }
p m

module NS
  class C
    def self.run
      yield
    end
  end
end
puts(NS::C.run { "scoped" })
k = NS::C.run { 5 }
p k
