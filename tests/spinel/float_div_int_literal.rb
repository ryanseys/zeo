# Float arithmetic with an INTEGER operand is spelled as a double. The result
# is a float division either way -- C promotes it -- but with the divisor left
# an integer constant, `1.0 / 0` folded to gcc's -Wdiv-by-zero and the build
# stopped, where Ruby (and the same expression written `1.0 / 0.0`, or with a
# variable divisor) answers Infinity.
p(1.0 / 0)
p(-1.0 / 0)
p(0.0 / 0)
x = 1.0
p(x / 0)
p(1.0 / 2)
p(3 / 2.0)
p(1.0 + 2)
p(2 * 1.5)
p(7.5 - 2)
p(2.0 ** 3)
begin
  p(1 / 0)
rescue ZeroDivisionError => e
  p e.message
end
begin
  p(1.0 % 0)
rescue ZeroDivisionError => e
  p e.message
end
p(1.0 / 0 > 10 ** 100)
