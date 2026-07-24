# Method-level rescue: when a method body is a begin/rescue, the
# rescue body's last expression should become the method's return
# value. CRuby semantics: `def f; X; rescue; Y; end` returns X if
# X succeeds, Y if X raises and the rescue catches.
#
# This test pins int-returning shapes only. String-returning method
# bodies with begin/rescue need a separate fix to method return-type
# inference (which currently picks int as the default for BeginNode-
# bodied methods regardless of the begin/rescue branch types).

# Bare rescue catches and returns fallback
def safe(n)
  raise "boom" if n < 0
  n
rescue
  -1
end
puts safe(5)
puts safe(-1)

# Typed rescue catches matching class and returns from rescue body.
# Use explicit raise rather than Integer() since spinel's Integer()
# doesn't raise on unparseable input — separate gap, not under test.
def parse(s)
  raise ArgumentError, "bad" if s == "nope"
  s.to_i
rescue ArgumentError
  -999
end
puts parse("42")
puts parse("nope")

# Rescue body with multiple statements: last is the return value
def lookup(k)
  raise "missing" if k == "x"
  k.length
rescue
  msg = "fallback"
  msg.length
end
puts lookup("hello")
puts lookup("x")
