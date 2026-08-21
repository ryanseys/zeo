# `private_constant` and `public_constant` are RUN-TIME flags, and each runs
# where it is written -- a read between two directives sees whichever had
# run by then. zeo applied the whole program's directives at STARTUP, so a
# read before a `private_constant` raised and a read before a later
# `public_constant` did not.

module Z
  D = 1
  private_constant :D
end
begin
  p Z::D
rescue NameError => e
  puts "1: #{e.message}"
end
p defined?(Z::D)

module Z
  public_constant :D
end
p Z::D
p defined?(Z::D)

module Y
  E = 2
end
p Y::E
p defined?(Y::E)

module Y
  private_constant :E
end
begin
  p Y::E
rescue NameError => e
  puts "2: #{e.message}"
end
p Y.constants

# The directive is still an expression, and its value is the module it
# applied to.
v = module N
  Inner = 3
  private_constant :Inner
end
p v
