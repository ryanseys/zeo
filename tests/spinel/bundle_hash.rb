# Bundled tests:
#   - empty_hash_promote
#   - hash
#   - hash_delete_order
#   - hash_keys_each
#   - hash_transform_keys
#   - hash_transform_values
#   - int_str_hash

# === empty_hash_promote ===
def t_empty_hash_promote
  # Empty hash literal whose first []= write pins a different key/value
  # type pair than the str_int_hash default. Pre-fix: declaration ran
  # before any []= so `h = {}; h[1] = "one"` got declared as
  # sp_StrIntHash and the int-keyed []= failed to compile.
  
  # Empty -> string keys, int values (matches str_int_hash default; works pre-fix)
  h1 = {}
  h1["k"] = 1
  h1["m"] = 2
  puts h1["k"]
  puts h1["m"]
  puts h1.length
  
  # Empty -> string keys, string values
  h2 = {}
  h2["x"] = "alpha"
  h2["y"] = "beta"
  puts h2["x"]
  puts h2["y"]
  puts h2.length
  
  # Empty -> int keys, string values
  h3 = {}
  h3[1] = "one"
  h3[2] = "two"
  puts h3[1]
  puts h3[2]
  puts h3.length
  
  # Empty -> sym keys, int values
  h4 = {}
  h4[:a] = 10
  h4[:b] = 20
  puts h4[:a]
  puts h4[:b]
  puts h4.length
  
  # Empty -> sym keys, string values
  h5 = {}
  h5[:name] = "ada"
  h5[:role] = "scientist"
  puts h5[:name]
  puts h5[:role]
  puts h5.length
end
t_empty_hash_promote

# === hash ===
def t_hash
  # Test Hash support
  
  # Hash literal
  h = {}
  h["one"] = 1
  h["two"] = 2
  h["three"] = 3
  puts h["two"]    # 2
  puts h.length    # 3
  
  # Iteration
  h.each do |k, v|
    puts k
  end
  
  # keys
  puts h.keys.length  # 3
  
  # has_key?
  if h.has_key?("two")
    puts "found"
  end
  
  # delete
  h.delete("two")
  puts h.length  # 2
end
t_hash

# === hash_delete_order ===
def t_hash_delete_order
  # Regression test: Hash#delete must compact the insertion-order array
  # so that subsequent #keys / #values / each iteration skip the deleted
  # entry. The Robin Hood backing array was being repaired correctly on
  # delete but the parallel `order[]` array was not, so #keys returned the
  # stale key and #values returned a zero / NULL slot for it.
  
  # String keys
  sh = {"a" => 1, "b" => 2, "c" => 3}
  sh.delete("b")
  puts sh.keys.inspect          # ["a", "c"]
  puts sh.values.inspect        # [1, 3]
  puts sh.length                # 2
  
  # Symbol keys (the path that exposed the bug via PR #246)
  yh = {a: 1, b: 2, c: 3}
  yh.delete(:b)
  puts yh.keys.inspect          # [:a, :c]
  puts yh.values.inspect        # [1, 3]
  
  # Delete first / last entries (boundary cases for the shift loop)
  fh = {a: 1, b: 2, c: 3}
  fh.delete(:a)
  puts fh.keys.inspect          # [:b, :c]
  
  lh = {a: 1, b: 2, c: 3}
  lh.delete(:c)
  puts lh.keys.inspect          # [:a, :b]
end
t_hash_delete_order

# === hash_keys_each ===
def t_hash_keys_each
  # Test hash.keys.each fusion for all hash types
  
  # int_str_hash: integer keys
  h1 = {3 => "Fizz", 5 => "Buzz", 7 => "Bazz"}
  h1.keys.each do |ki|
    puts ki
  end
  # => 3 5 7
  
  # str_str_hash: string keys
  h2 = {"a" => "apple", "b" => "banana", "c" => "cherry"}
  h2.keys.each do |ks|
    puts ks
  end
  # => a b c
  
  # str_int_hash: string keys, integer values
  h3 = {"x" => 10, "y" => 20, "z" => 30}
  h3.keys.each do |ks2|
    puts ks2
  end
  # => x y z
  
  # body reads value via lookup
  total = 0
  {10 => "ten", 20 => "twenty", 30 => "thirty"}.keys.each do |n|
    total = total + n
  end
  puts total
  # => 60
  
  # common pattern: conditional accumulation
  map = {3 => "Fizz", 5 => "Buzz"}
  output = ""
  map.keys.each do |d|
    if 15 % d == 0
      output = output + map[d]
    end
  end
  puts output
  # => FizzBuzz
  
  # no block param
  count = 0
  {"a" => 1, "b" => 2}.keys.each do
    count = count + 1
  end
  puts count
  # => 2
end
t_hash_keys_each

# === hash_transform_keys ===
def t_hash_transform_keys
  # Hash#transform_keys for str_int_hash. The block runs once per key,
  # its return value becomes the new key. Mirrors transform_values'
  # shape (which already shipped) but on the key axis.
  
  # Identity transform — keys unchanged
  h1 = {"alpha" => 1, "beta" => 2}
  puts h1.transform_keys { |k| k }["alpha"]
  puts h1.transform_keys { |k| k }["beta"]
  
  # Upcase keys
  h2 = {"hello" => 10, "world" => 20}
  upper = h2.transform_keys { |k| k.upcase }
  puts upper["HELLO"]
  puts upper["WORLD"]
  puts upper.has_key?("hello")
  puts upper.has_key?("HELLO")
  
  # Concat suffix
  h3 = {"a" => 100, "b" => 200}
  suff = h3.transform_keys { |k| k + "_x" }
  puts suff["a_x"]
  puts suff["b_x"]
  
  # Empty hash transform
  empty = {}
  empty["k"] = 1
  empty.delete("k")
  puts empty.transform_keys { |k| k.upcase }.length
  
  # Length preserved
  big = {"one" => 1, "two" => 2, "three" => 3}
  puts big.transform_keys { |k| k + "!" }.length
end
t_hash_transform_keys

# === hash_transform_values ===
def t_hash_transform_values
  # Hash#transform_values across hash variants. The block runs once
  # per value, its return becomes the new value, keys and order
  # preserved. str_int_hash already shipped; this covers int_str_hash
  # and sym_int_hash.
  
  # === sym_int_hash variant ===
  # `{a: 1, b: 2}` parses as sym→int. transform_values keeps the
  # key set and feeds each value through the block. p on the result
  # uses the new sp_SymIntHash_inspect helper to print the hash in
  # Ruby's `{a: V, b: V, ...}` form.
  p({a: 1, b: 2}.transform_values { |v| v * 10 })
  p({x: 5, y: 10, z: 15}.transform_values { |v| v + 100 })
  puts({a: 1, b: 2, c: 3}.transform_values { |v| v * v }[:c])
  puts({foo: 7}.transform_values { |v| v - 2 }[:foo])
  
  # Non-destructive — original hash retains its values; result is a
  # fresh hash. transform_values must not mutate the receiver.
  hh = {a: 1, b: 2}
  hh2 = hh.transform_values { |v| v * 100 }
  p hh         # {a: 1, b: 2}
  p hh2        # {a: 100, b: 200}
  
  # === int_str_hash variant ===
  
  # Identity transform — values unchanged
  h1 = {1 => "alpha", 2 => "beta"}
  puts h1.transform_values { |v| v }[1]
  puts h1.transform_values { |v| v }[2]
  
  # Upcase values
  h2 = {1 => "hello", 2 => "world"}
  upper = h2.transform_values { |v| v.upcase }
  puts upper[1]
  puts upper[2]
  
  # String concat
  h3 = {1 => "a", 2 => "b", 3 => "c"}
  suff = h3.transform_values { |v| v + "!" }
  puts suff[1]
  puts suff[2]
  puts suff[3]
  
  # Length preserved across transform
  big = {10 => "one", 20 => "two", 30 => "three"}
  puts big.transform_values { |v| v + "?" }.length
  
  # Empty block maps every value to nil (CRuby parity).
  # For int_str_hash the value type is `const char *`; nil → NULL.
  empty = {1 => "alpha", 2 => "beta"}.transform_values { }
  puts empty[1].nil?
  puts empty[2].nil?
  puts empty.length
end
t_hash_transform_values

# === int_str_hash ===
def t_int_str_hash
  # Test integer-keyed string-valued hash (sp_IntStrHash)
  # Regression: previously segfaulted — codegen emitted sp_StrStrHash,
  # causing sp_str_hash() to dereference integer keys as pointers.
  
  # Literal creation and lookup
  h = {3 => "Fizz", 5 => "Buzz", 7 => "Bazz"}
  puts h[3]          # Fizz
  puts h[5]          # Buzz
  puts h[7]          # Bazz
  puts h[1]          # (empty)
  
  # has_key?
  puts h.has_key?(3)   # true
  puts h.has_key?(4)   # false
  
  # length
  puts h.length        # 3
  
  # keys returns int array
  puts h.keys.length   # 3
  puts h.keys[0]       # 3
  
  # values
  puts h.values[0]     # Fizz
  
  # keys.each iteration
  h.keys.each do |k|
    puts k
  end
  
  # each iteration (key, value)
  h.each do |k, v|
    puts "#{k}:#{v}"
  end
  
  # []= assignment
  h[15] = "FizzBuzz"
  puts h[15]           # FizzBuzz
  puts h.length        # 4
  
  # fetch with default
  puts h.fetch(3, "none")   # Fizz
  puts h.fetch(99, "none")  # none
  
  # FizzBuzz pattern — the original failing case
  map = {3 => "Fizz", 5 => "Buzz"}
  (1..15).each do |x|
    out = ""
    map.keys.each do |k|
      out += map[k] if x % k == 0
    end
    out = x.to_s if out.empty?
    puts out
  end
end
t_int_str_hash

