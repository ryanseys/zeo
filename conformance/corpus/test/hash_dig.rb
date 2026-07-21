# Hash#dig: nested key traversal across hash variants.

# 1. Single-key form on each hash variant — alias for `[]`.
sih = {"one" => 1, "two" => 2}
puts sih.dig("one")          # 1
puts sih.dig("two")           # 2

ssh = {"a" => "alpha", "b" => "beta"}
puts ssh.dig("a")             # alpha
puts ssh.dig("b")             # beta

ish = {1 => "x", 2 => "y"}
puts ish.dig(1)               # x
puts ish.dig(2)               # y

syih = {a: 10, b: 20}
puts syih.dig(:a)             # 10
puts syih.dig(:b)             # 20

# 2. Multi-key on a sym_poly_hash with a sym_poly_hash leaf.
nested = { user: { name: "Alice", age: 30 }, count: 7 }
puts nested.dig(:user, :name) # Alice
puts nested.dig(:user, :age)  # 30
puts nested.dig(:count)       # 7

# 3. Multi-key on a sym_poly_hash whose inner hash is sym_int_hash.
config = { limits: { read: 100, write: 50 }, version: 2 }
puts config.dig(:limits, :read)  # 100
puts config.dig(:limits, :write) # 50
puts config.dig(:version)        # 2

# 4. Multi-key on a sym_poly_hash whose inner hash is sym_str_hash.
labels = { en: { hello: "hi", bye: "bye" }, ja: { hello: "konnichiwa", bye: "sayonara" } }
puts labels.dig(:en, :hello)    # hi
puts labels.dig(:ja, :bye)      # sayonara

# 5. Three-deep nesting.
deep = { a: { b: { c: 42 } } }
puts deep.dig(:a, :b, :c)       # 42

# 6. Missing keys produce nil.
miss = { a: { b: 1 } }
puts miss.dig(:nope)            # (blank)
puts miss.dig(:a, :nope)        # (blank)
# CRuby raises TypeError on `int.dig(:c)`; spinel returns nil. Not
# probed here so `.expected` stays comparable to CRuby.

# 7. Same shape for str_poly_hash.
sph = { "user" => { "name" => "Bob", "age" => 25 } }
puts sph.dig("user", "name")    # Bob
puts sph.dig("user", "age")     # 25
puts sph.dig("missing")         # (blank)

# 8. Real 0 / "" leaves vs missing keys: `p` distinguishes 0 / "" / nil.
zh = { a: { b: 0 } }
p zh.dig(:a, :b)                # 0
p zh.dig(:a, :missing)          # nil
zs = { a: { b: "" } }
p zs.dig(:a, :b)                # ""
p zs.dig(:a, :missing)          # nil

# 9. str_poly_hash with str_int_hash and str_str_hash leaves.
counts = { "a" => { "x" => 1, "y" => 2 } }
puts counts.dig("a", "x")       # 1
p counts.dig("a", "missing")    # nil
labs2 = { "a" => { "x" => "hi" } }
puts labs2.dig("a", "x")        # hi
p labs2.dig("a", "missing")     # nil

# 10. Mid-walk into a non-hash leaf.
flat = { a: 1 }
p flat.dig(:a, :b)              # nil

# 11. Each key expression evaluates exactly once across the dig walk.
def k_first
  puts "k1"
  :a
end
def k_second
  puts "k2"
  :b
end
side = { a: { b: 99 } }
puts side.dig(k_first, k_second)

# 12. Static key-type mismatch returns nil (CRuby compat).
sym_h = {a: 1}
p sym_h.dig("a")                # nil  (string key on sym hash)
str_h = {"a" => 1}
p str_h.dig(:a)                 # nil  (sym key on str hash)
mismatch = { a: { b: 1 } }
p mismatch.dig("a", :b)         # nil  (multi-key first-key mismatch)
