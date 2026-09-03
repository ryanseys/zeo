# A `while`/`until`/`loop`/`for` written where a value is READ: the loop
# itself is worth nil (`for` its collection), and a `break v` its own.
r = while true
  break :val
end
p r
i = 0
q = while i < 3
  i += 1
end
p q
n = 0
z = until n >= 2
  n += 1
  break n if n == 2
end
p z
f = for x in [1,2,3]
  break :early if x == 2
end
p f
g = for y in [1,2]
end
p g
l = loop do
  break 42
end
p l
h = [1,2].map { |v| (while false; end) || v }
p h
__END__
:val
nil
2
:early
[1, 2]
42
[1, 2]
