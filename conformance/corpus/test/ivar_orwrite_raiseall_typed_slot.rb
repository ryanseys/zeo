# `@x ||= recv.map { ... }` where recv is typed poly/unknown produces a
# raise-all (sp_RbVal) RHS, but @x's slot was typed concretely (an array)
# from another assignment. The pointer-backed ivar or-write must keep the
# raise for effect and yield the slot's typed NULL so the C compiles, then
# raise NoMethodError at run time like CRuby (#2457, Family 1).
class Store
  def taggings; @data; end
  def seed(d); @memo = d; end
  def compute
    @memo ||= taggings.reject { |x| false }.map { |x| x }
  end
end
s = Store.new
begin
  p s.compute
rescue NoMethodError => e
  p e.class
end
s2 = Store.new
s2.seed([10, 20])
p s2.compute
