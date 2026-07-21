# yjit-bench: object-new - object allocation performance
# Ported from https://github.com/Shopify/yjit-bench

class SimpleObj
  def initialize
    @x = 0
  end

  def x
    @x
  end
end

sum = 0
i = 0
while i < 1000000
  obj = SimpleObj.new
  sum = sum + obj.x
  i = i + 1
end
puts sum
