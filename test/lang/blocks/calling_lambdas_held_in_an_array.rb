# Each element called with an Integer, and one taking a String through a
# parameter whose type is not known.
# (spinel issue #2883)
callbacks = []
callbacks << ->(e) { p e }
callbacks << ->(e) { p e * 10 }
callbacks.each { |cb| cb.call(5) }

# string arg through a poly-param proc read from a container
fns = []
fns << ->(s) { p s.upcase }
fns.each { |f| f.call("hi") }
__END__
5
50
"HI"
