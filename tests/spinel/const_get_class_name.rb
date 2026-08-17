# `Object.const_get(:Foo)` where Foo is a class or module the program defines:
# the lookup knew only VALUE constants, so it raised NameError for a constant
# the program does define.
module M
  X = 5
  class Inner
    def self.hi = "hi"
  end
end
class Top
  def self.v = 7
end
k = Object.const_get(:Top)
p k.v
p Object.const_get("Top").v
p Object.const_get(:M)::X
p M.const_get(:X)
p M.const_get(:Inner).hi
p Object.const_get(:Top) == Top
p Object.const_get(:Top).name
begin
  Object.const_get(:Nope)
rescue NameError => e
  p e.message
end
