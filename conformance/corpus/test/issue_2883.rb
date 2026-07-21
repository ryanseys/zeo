callbacks = []
callbacks << ->(e) { p e }
callbacks << ->(e) { p e * 10 }
callbacks.each { |cb| cb.call(5) }

# string arg through a poly-param proc read from a container
fns = []
fns << ->(s) { p s.upcase }
fns.each { |f| f.call("hi") }
