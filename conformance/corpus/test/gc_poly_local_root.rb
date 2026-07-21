# A poly (sp_RbVal) local assigned from a method whose return type
# unifies to poly must be GC-rooted, or a collection while it's live
# frees the object it holds (use-after-free).
class A; def v=(x); @v = x; end; def v; @v; end; end
class B; def v=(x); @v = x; end; def v; @v; end; end
def pick(n)
  if n == 0
    A.new
  else
    B.new
  end
end
def churn(k); s = ""; i = 0; while i < k; s = s + i.to_s + ","; i += 1; end; s.length; end
obj = pick(1)
obj.v = "keep_" + 5.to_s
GC.start
churn(8000)
GC.start
puts obj.v
