# Integer#<< / #>> accept a Float count (truncated via to_int); the bitwise
# &/|/^ and the arithmetic ops with a non-coercible argument raise TypeError.
p(5 << 2.0)
p(5 << 2.9)
p(100 >> 1.9)
p(10 << 3)
begin; 5 & 2.0; rescue => e; p e.class; end
begin; 5 % "x"; rescue => e; p [e.class, e.message]; end
begin; 5 + "x"; rescue => e; p e.class; end
begin; 5 * :sym; rescue => e; p e.class; end
begin; 5 % [1]; rescue => e; p e.class; end
p(5 % 3)
p(7 + 2.0)
