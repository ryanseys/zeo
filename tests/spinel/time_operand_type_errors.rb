# Time + Time is a TypeError and Time#between?/#clamp with non-Time bounds is
# an ArgumentError; both read the operand as a number and did not build
# (#3865).
t = Time.utc(2020, 1, 2)
u = Time.utc(2020, 1, 3)
begin; p(t + t); rescue => e; p e.class; end
begin; p t.between?(1, 2); rescue => e; p e.class; end
begin; p t.clamp(1, 2); rescue => e; p e.class; end
p((t + 60).to_i - t.to_i)
p t.between?(Time.utc(2020, 1, 1), u)
p((u - t).to_i)
