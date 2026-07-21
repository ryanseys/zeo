# setivar_young benchmark (from yjit-bench)

class TheClass
  def initialize
    @v0 = 1
    @v1 = 2
    @v3 = 3
    @levar = TheClass.new_inner
    @tag = "young"
  end

  def self.new_inner
    0
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

tc = TheClass.new
puts tc.set_value_loop
puts "done"
