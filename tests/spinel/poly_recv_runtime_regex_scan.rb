# A poly receiver scanning against a pattern the compiler cannot resolve to a
# literal.
#
# The poly-receiver arm gated on the pattern being statically visible, so an
# inline Regexp.new, a local holding one, or an interpolated literal fell past
# the handler to the unresolved-call gate and raised NoMethodError on a String
# that plainly answers #scan. The String-receiver arm has taken those shapes
# since #3389; the gate itself still earns its keep, so it stays -- a non-String
# receiver must raise rather than be stringified into an empty scan.

PAT = /(\d)(\w)/
mixed = ["a1b2", 7]
poly = mixed[0]

p poly.scan(/\d/)
p poly.scan(PAT)

lpat = /(\d)(\w)/
p poly.scan(lpat)

p poly.scan(Regexp.new("\\d"))
rt = Regexp.new("(\\d)(\\w)")
p poly.scan(rt)
d = "\\d"
p poly.scan(/#{d}/)
p poly.scan("1")

n = mixed[1]
begin
  n.scan(/\d/)
  p "no raise"
rescue NoMethodError
  p "NoMethodError"
end

begin
  n.scan(Regexp.new("\\d"))
  p "no raise"
rescue NoMethodError
  p "NoMethodError"
end
