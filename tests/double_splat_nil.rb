# `**nil` in a call is ruby's "pass no keywords" spelling -- a no-op, both as
# a literal and through a nil-valued variable. zeo lowers every `**expr` splat
# through a to-Hash conversion, so it raises TypeError ("no implicit
# conversion of nil into Hash") before the call happens.
def kw(a, b: 2, **rest)
  [a, b, rest]
end

puts kw(1, **nil).inspect
h = nil
puts kw(2, **h).inspect
