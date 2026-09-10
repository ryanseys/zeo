# A throw with no matching catch is an UncaughtThrowError whose #tag and #value are the throw's arguments.
p(begin; throw :nope, 42; rescue => e; e.class; end)
v = begin; throw :y; rescue UncaughtThrowError => e; e.tag; end
p v
w = begin; throw :z, 7; rescue UncaughtThrowError => e; e.value; end
p w
__END__
UncaughtThrowError
:y
7
