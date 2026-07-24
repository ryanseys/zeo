# reduce with a Symbol accumulator whose block value is read from a nested Hash
# (poly): the poly block value coerces back to the sp_sym accumulator slot.
T = { a: { "0" => :a, "1" => :b }, b: { "0" => :a, "1" => :b } }
p "01".chars.reduce(:a) { |s, c| T[s][c] }
p "10".chars.reduce(:a) { |s, c| T[s][c] }
