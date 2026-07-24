# A poly local live across a rescue (setjmp scope) with a GC: it must
# be both volatile (longjmp-stable) and GC-rooted.
class A; def v=(x); @v = x; end; def v; @v; end; end
class B; def v=(x); @v = x; end; def v; @v; end; end
def pick(n); n == 0 ? A.new : B.new; end
def churn(k); s = ""; i = 0; while i < k; s = s + i.to_s + ","; i += 1; end; s.length; end
obj = pick(1)
obj.v = "keep_" + 9.to_s
begin
  GC.start
  churn(8000)
  raise "boom"
rescue => e
  GC.start
end
puts obj.v
