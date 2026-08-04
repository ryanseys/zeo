def each001(h001); h001.each { |k001, v001| nil }; end
p each001({ "a" => 1 })

def each002(a002); a002.each { |x002| nil }; end
p each002([1, 2])

def each003(r003); r003.each { |x003| nil }; end
p each003(1..2)

def each004(n004); n004.times { |i004| nil }; end
p each004(2)

l005 = ->(h005) { h005.each { |k005, v005| nil } }
p l005.call({ "a" => 1 })

def each006(h006); if true then h006.each { |k006, v006| nil } else h006 end; end
p each006({ "a" => 1 })

def ep(h); h.each_pair { |k, v| nil }; end
p ep({ "a" => 1 })

def ek(h); h.each_key { |k| nil }; end
p ek({ "a" => 1 })

def ev(h); h.each_value { |v| nil }; end
p ev({ "a" => 1 })

def ewi(a); a.each_with_index { |x, i| nil }; end
p ewi([1, 2])

def re(a); a.reverse_each { |x| nil }; end
p re([1, 2])

def es(a); a.each_slice(2) { |x| nil }; end
p es([1, 2, 3])

def up(n); n.upto(3) { |i| nil }; end
p up(1)

def st(n); n.step(5, 2) { |i| nil }; end
p st(1)

# these were already right; keep them pinned
def ec(s); s.each_char { |ch| nil }; end
p ec("ab")

def mp(a); a.map { |x| x * 2 }; end
p mp([1, 2])

def each017(h017); return h017.each { |k017, v017| nil }; end
p each017({ "a" => 1 })

def each018(h018); r018 = h018.each { |k018, v018| nil }; r018; end
p each018({ "a" => 1 })
