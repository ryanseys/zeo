# yjit-bench: setivar - instance variable write performance
# Ported from https://github.com/Shopify/yjit-bench

class TheClass
  def initialize
    @v0 = 1
    @v1 = 2
    @v3 = 3
    @levar = 1
  end

  def set_value_loop
    i = 0
    while i < 1000000
      @levar = i
      @levar = i
      @levar = i
      @levar = i
      @levar = i
      @levar = i
      @levar = i
      @levar = i
      @levar = i
      @levar = i
      i = i + 1
    end
    @levar
  end
end

obj = TheClass.new
result = obj.set_value_loop
puts result
