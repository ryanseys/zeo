# clamp(lo, hi) accepts nil for an open (unbounded) side on Integer and Float
# receivers: a nil lo skips the lower comparison, a nil hi the upper.
p 5.clamp(1, nil)
p 0.clamp(1, nil)
p 5.clamp(nil, nil)
p 12.clamp(nil, 9)
p 5.0.clamp(1.0, nil)
p 5.0.clamp(nil, 3.0)
p 2.0.clamp(nil, nil)
p 0.0.clamp(1.0, nil)
