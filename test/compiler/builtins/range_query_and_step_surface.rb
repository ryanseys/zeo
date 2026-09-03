# Every `Range` query and `#step` shape, against ruby 4.0.6: the closed forms
# an unbounded range answers without walking, the negative-count refusals, and
# the bad-stride messages -- which all come from `begin + step`.
def t(label)
  begin
    p [label, yield]
  rescue Exception => e
    p [label, e.class.name, e.message]
  end
end
t("rev endless")  { (1..).reverse_each.first(2) }
t("rev beginless"){ (..5).reverse_each.first(3) }
t("rev int")      { (1..5).reverse_each.to_a }
t("rev str")      { ("a".."e").reverse_each.to_a }
t("rev float")    { (1.0..5.0).reverse_each.to_a }
t("rev beginfl")  { (..5.0).reverse_each.first(2) }
t("last -1")      { (1..5).last(-1) }
t("min -1")       { (1..5).min(-1) }
t("max -1")       { (1..5).max(-1) }
t("first -1")     { (1..5).first(-1) }
t("minmax endless"){ (1..).minmax }
t("minmax beginless"){ (..5).minmax }
t("minmax float") { (1.0..5.0).minmax }
t("minmax floatx"){ (1.0...5.0).minmax }
t("count endless"){ (1..).count }
t("count beginless"){ (..5).count }
t("to_a beginless"){ (..5).to_a }
t("minmax block") { (1..5).minmax { |a,b| b<=>a } }
t("eql same")     { (1..5).eql?(1..5) }
t("eql float")    { (1..5).eql?(1.0..5.0) }
t("eqeq float")   { (1..5) == (1.0..5.0) }
t("eql int")      { (1..5).eql?(1) }
t("cover r1")     { (1..5).cover?(1...6) }
t("cover r2")     { (1..5).cover?(2..9) }
t("cover r3")     { (1..5).cover?(1.0..5.0) }
t("cover r4")     { (1..5).cover?(1.0...5.5) }
t("cover r5")     { (1...5).cover?(1..4) }
t("=== range")    { (1..5) === (1.0..5.0) }
t("include range"){ (1..5).include?(1..2) }
t("cover beginless"){ (1..5).cover?(nil..3) }
t("cover endless"){ (1..5).cover?(2..) }
t("beginless cover"){ (nil..5).cover?(1..3) }
t("hash")         { (1..5).hash == (1..5).hash }
t("step size inf"){ (1..).step(2).size }
t("step size")    { (1..5).step(2).size }
t("step noarg")   { (1..3).step.to_a }
t("step str")     { ("a".."e").step(2).to_a }
t("pct")          { ((1..10) % 3).to_a }
t("step neg")     { (1..10).step(-1).inspect }
t("step beginless"){ (..5).step(2) { |x| x } }
t("step badf")    { (1.0..10.0).step("x").to_a }
t("step bads")    { ("a".."e").step(:s).to_a }
t("step badi")    { (1..10).step(nil).to_a }

# The `min`/`max`/`minmax` closed forms: `range.c` READS the endpoints rather
# than walking, so an empty, exclusive, Float, endless or beginless range
# answers without an iteration a monkey-patched `each` could reach.
t("min i"){ (1..4).min }; t("max i"){ (1..4).max }
t("min ix"){ (1...4).min }; t("max ix"){ (1...4).max }
t("min s"){ ("a".."e").min }; t("max s"){ ("a".."e").max }
t("min sx"){ ("a"..."e").min }; t("max sx"){ ("a"..."e").max }
t("min f"){ (1.0..5.0).min }; t("max f"){ (1.0..5.0).max }
t("min fx"){ (1.0...5.0).min }; t("max fx"){ (1.0...5.0).max }
t("min empty"){ (5..1).min }; t("max empty"){ (5..1).max }
t("min emptyx"){ (5...1).min }; t("max emptyx"){ (5...1).max }
t("min same x"){ (3...3).min }; t("max same x"){ (3...3).max }
t("min endless"){ (1..).min }; t("max beginless"){ (..5).max }
t("max beginless x"){ (...5).max }
t("min n"){ (1..5).min(2) }; t("max n"){ (1..5).max(2) }
t("min blk"){ (1..5).min { |a,b| b<=>a } }; t("max blk"){ (1..5).max { |a,b| b<=>a } }
t("minmax"){ (1..5).minmax }; t("minmax s"){ ("a".."e").minmax }
t("minmax x"){ (1...5).minmax }; t("minmax sx"){ ("a"..."e").minmax }
t("max fx begin"){ (1.0...5).max }
t("max str excl"){ ("a"..."e").max }
t("max n beginless"){ (..5).max(2) }
t("max n"){ (1..9).max(3) }
t("max n str"){ ("a".."e").max(2) }
t("max n excl"){ (1...9).max(3) }
t("max 0"){ (1..9).max(0) }
t("max big"){ (1..3).max(10) }
t("max n float"){ (1.0..5.0).max(2) }
t("min n"){ (1..9).min(3) }
t("min n str"){ ("a".."e").min(2) }
t("last n"){ (1..9).last(3) }
t("first n"){ (1..9).first(3) }
t("minmax beginless int"){ (..5).minmax }
__END__
["rev endless", "TypeError", "can't iterate from NilClass"]
["rev beginless", [5, 4, 3]]
["rev int", [5, 4, 3, 2, 1]]
["rev str", ["e", "d", "c", "b", "a"]]
["rev float", "TypeError", "can't iterate from Float"]
["rev beginfl", "TypeError", "can't iterate from NilClass"]
["last -1", "ArgumentError", "negative array size"]
["min -1", "ArgumentError", "negative array size (or size too big)"]
["max -1", "ArgumentError", "negative array size (or size too big)"]
["first -1", "ArgumentError", "negative array size (or size too big)"]
["minmax endless", "RangeError", "cannot get the maximum of endless range"]
["minmax beginless", "RangeError", "cannot get the minimum of beginless range"]
["minmax float", [1.0, 5.0]]
["minmax floatx", "TypeError", "cannot exclude non Integer end value"]
["count endless", Infinity]
["count beginless", Infinity]
["to_a beginless", "TypeError", "can't iterate from NilClass"]
["minmax block", [5, 1]]
["eql same", true]
["eql float", false]
["eqeq float", true]
["eql int", false]
["cover r1", true]
["cover r2", false]
["cover r3", true]
["cover r4", false]
["cover r5", true]
["=== range", false]
["include range", false]
["cover beginless", false]
["cover endless", false]
["beginless cover", true]
["hash", true]
["step size inf", Infinity]
["step size", 3]
["step noarg", [1, 2, 3]]
["step str", ["a", "c", "e"]]
["pct", [1, 4, 7, 10]]
["step neg", "((1..10).step(-1))"]
["step beginless", "ArgumentError", "#step iteration for beginless ranges is meaningless"]
["step badf", "TypeError", "String can't be coerced into Float"]
["step bads", "TypeError", "no implicit conversion of Symbol into String"]
["step badi", "TypeError", "nil can't be coerced into Integer"]
["min i", 1]
["max i", 4]
["min ix", 1]
["max ix", 3]
["min s", "a"]
["max s", "e"]
["min sx", "a"]
["max sx", "d"]
["min f", 1.0]
["max f", 5.0]
["min fx", 1.0]
["max fx", "TypeError", "cannot exclude non Integer end value"]
["min empty", nil]
["max empty", nil]
["min emptyx", nil]
["max emptyx", nil]
["min same x", nil]
["max same x", nil]
["min endless", 1]
["max beginless", 5]
["max beginless x", 4]
["min n", [1, 2]]
["max n", [5, 4]]
["min blk", 5]
["max blk", 1]
["minmax", [1, 5]]
["minmax s", ["a", "e"]]
["minmax x", [1, 4]]
["minmax sx", ["a", "d"]]
["max fx begin", "TypeError", "cannot exclude end value with non Integer begin value"]
["max str excl", "d"]
["max n beginless", [5, 4]]
["max n", [9, 8, 7]]
["max n str", ["e", "d"]]
["max n excl", [8, 7, 6]]
["max 0", []]
["max big", [3, 2, 1]]
["max n float", "TypeError", "can't iterate from Float"]
["min n", [1, 2, 3]]
["min n str", ["a", "b"]]
["last n", [7, 8, 9]]
["first n", [1, 2, 3]]
["minmax beginless int", "RangeError", "cannot get the minimum of beginless range"]
