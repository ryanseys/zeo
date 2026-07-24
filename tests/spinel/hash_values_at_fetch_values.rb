# Hash#values_at / #fetch_values on typed hashes. Each key is looked up
# and the values collected into an array; values_at yields nil for a
# missing key, fetch_values raises KeyError. Results box into a
# poly_array so mixed value types and nil coexist.
h = {a: 1, b: 2, c: 3}
p h.values_at(:a, :c)
p h.values_at(:a, :c, :d)
p h.fetch_values(:a, :b, :c)

s = {"x" => 10, "y" => 20}
p s.values_at("x", "z")

ss = {a: "one", b: "two"}
p ss.values_at(:a, :b, :c)

pp = {a: 1, b: "two", c: nil}
p pp.values_at(:a, :b, :c, :d)

begin
  h.fetch_values(:a, :z)
rescue KeyError
  puts "KeyError raised"
end
