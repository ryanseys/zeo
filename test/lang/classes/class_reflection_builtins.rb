p Enumerator.name
p Enumerator.ancestors
Pt = Struct.new(:x)
p Pt.name
p Pt.ancestors
p Pt.superclass
p Rational.name
p Regexp.name
p Complex.ancestors
p Range.ancestors
p Complex.superclass
p Integer.ancestors
p Symbol.ancestors
__END__
"Enumerator"
[Enumerator, Enumerable, Object, Kernel, BasicObject]
"Pt"
[Pt, Struct, Enumerable, Object, Kernel, BasicObject]
Struct
"Rational"
"Regexp"
[Complex, Numeric, Comparable, Object, Kernel, BasicObject]
[Range, Enumerable, Object, Kernel, BasicObject]
Numeric
[Integer, Numeric, Comparable, Object, Kernel, BasicObject]
[Symbol, Comparable, Object, Kernel, BasicObject]
