# A poly local holding a non-pointer (int) value must be marked
# tag-aware: rooting routes through sp_mark_rbval, which skips INT
# rather than dereferencing the boxed integer as a pointer.
class A; def v; 1; end; end
def churn(k); s = ""; i = 0; while i < k; s = s + i.to_s + ","; i += 1; end; s.length; end
arr = [A.new, 100 + 1]
x = arr[1]
GC.start
churn(8000)
GC.start
p x
