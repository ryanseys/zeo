# yjit-bench: getivar - instance variable read performance
# Ported from https://github.com/Shopify/yjit-bench

class TheClass
  def initialize
    @v0 = 1
    @v1 = 2
    @v2 = 3
    @levar = 1
  end

  def get_value_loop
    sum = 0
    i = 0
    while i < 1000000
      sum = sum + @levar
      sum = sum + @levar
      sum = sum + @levar
      sum = sum + @levar
      sum = sum + @levar
      sum = sum + @levar
      sum = sum + @levar
      sum = sum + @levar
      sum = sum + @levar
      sum = sum + @levar
      i = i + 1
    end
    sum
  end
end

obj = TheClass.new
result = obj.get_value_loop
puts result
