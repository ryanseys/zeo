# String#to_c raised where CRuby answers (0+0i). One parser was serving two
# contracts: Kernel#Complex(str) must consume the whole string or raise, while
# String#to_c reads a LEADING complex and ignores the rest. String#to_r has had
# that pair (sp_str_to_r / sp_str_to_r_strict) all along; to_c had only the
# strict half and #to_c was pointed at it.
#
# And sp_poly_to_r_m had no String arm, though sp_poly_to_c_m has carried one
# all along -- so a String that widened to poly answered NoMethodError, which
# is what `v&.to_r` is.

# String#to_c: a leading complex, the rest ignored, (0+0i) when there is none
p "abc".to_c
p "".to_c
p "5x".to_c
p "1+2i".to_c
p "3".to_c
p "2.5".to_c
p "  7  ".to_c

# String#to_r, the sibling that was already right
p "abc".to_r
p "3/4".to_r
p "1.5".to_r
p "12".to_r
p "5x".to_r
p "".to_r

# Kernel#Complex / #Rational keep the strict contract
def try
  yield
rescue ArgumentError
  "ArgumentError"
end

p try { Complex("abc") }
p try { Complex("1+") }
p try { Complex("5x") }
p try { Complex("2+3i") }
p try { Rational("abc") }
p try { Rational("3/4") }
p Complex("abc", exception: false)
p Rational("abc", exception: false)

# through a poly receiver, which is where the missing String arm showed
def as_r(v) = v&.to_r
def as_c(v) = v&.to_c

p as_r("3/4")
p as_r("abc")
p as_r(nil)
p as_r(5)
p as_r(1.5)
p as_c("1+2i")
p as_c("abc")
p as_c(nil)
p as_c(5)
