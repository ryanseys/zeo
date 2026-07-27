g = ->() { loop { break }; 42 }
p g.call
h = ->() { r = []; loop { break }; [r, 1] }
p h.call
k = ->() { v = 0; loop { v += 1; break if v > 2 }; v }
p k.call
pr = proc { loop { break }; :done }
p pr.call
