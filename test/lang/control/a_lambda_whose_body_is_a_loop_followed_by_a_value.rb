# The loop breaks and the lambda answers the value after it, for three shapes.
# (spinel issue #3310)
g = ->() { loop { break }; 42 }
p g.call
h = ->() { r = []; loop { break }; [r, 1] }
p h.call
k = ->() { v = 0; loop { v += 1; break if v > 2 }; v }
p k.call
pr = proc { loop { break }; :done }
p pr.call
__END__
42
[[], 1]
3
:done
