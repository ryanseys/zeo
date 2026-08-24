# `Time.allocate` answers a Time whose instant was never set, and every row
# that reads the instant refuses it -- CRuby's `uninitialized Time`.
#
# zeo used to raise `allocator undefined for Time` at the `allocate` itself,
# so the refusal came one step early and under the wrong message. The marker
# costs no field: `RTime::den` is a rational denominator and is positive for
# every real instant, so zero is free.
#
# Only the rows that READ the instant refuse. `class` and `frozen?` answer,
# and `instance_variables` is empty -- an allocated Time is an ordinary object
# until something asks it what time it is.
t = Time.allocate
%w[to_i to_f year month day hour min sec inspect to_s hash usec nsec].each do |m|
  begin
    p [m, t.send(m)]
  rescue => e
    p [m, e.class, e.message]
  end
end
p ["frozen?", t.frozen?]
p ["class", t.class]
p ["ivars", t.instance_variables]

# Two different blank Times cannot even be compared: `==` reads both instants.
a = Time.allocate
b = Time.allocate
p ["a==b", (a == b rescue [$!.class, $!.message])]
p ["a<=>b", (a <=> b rescue [$!.class, $!.message])]
p ["eql?", (a.eql?(b) rescue [$!.class, $!.message])]
p ["dup", (a.dup.class rescue [$!.class, $!.message])]

# An initialized Time is unaffected.
real = Time.at(0).utc
p [real.to_i, real.year, real.utc?]
