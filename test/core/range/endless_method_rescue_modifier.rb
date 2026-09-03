class Unsafe
  def op(n)
    raise "negative" if n < 0
    n * 2
  end
end
class Calc
  def safe_op(n) = Unsafe.new.op(n) rescue -1
end
c = Calc.new
puts c.safe_op(5)
puts c.safe_op(-5)
__END__
10
-1
