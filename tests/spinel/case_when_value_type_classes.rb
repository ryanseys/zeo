# `when Rational` / `when Complex` (and Regexp, Proc, Time) have a builtin
# cls_id but no class-table entry, so the arm used to compile to a constant
# false and never matched; `Klass === poly` answered false the same way (#3959).
def kind(v)
  case v
  when Integer then :int
  when Rational then :rat
  when Complex then :cpx
  when Regexp then :re
  when Proc then :pr
  else :other
  end
end
p kind(3)
p kind(Rational(1, 2))
p kind(Complex(1, 2))
p kind(/x/)
p kind(proc { 1 })
p kind("s")

def numeric?(v)
  case v
  when Numeric then :num
  else :other
  end
end
p numeric?(Rational(1, 2))
p numeric?(Complex(1, 2))
p numeric?(2**80)
p numeric?(1.5)
p numeric?("s")

def flags(v)
  [v.is_a?(Rational), v.is_a?(Complex), v.is_a?(Numeric), v.is_a?(Regexp),
   Rational === v, Complex === v, Regexp === v]
end
p flags(Rational(1, 2))
p flags(Complex(1, 2))
p flags(/x/)
p flags(3)
