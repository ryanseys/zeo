# Bundled tests:
#   - sym_array
#   - sym_array_empty_push
#   - sym_array_full
#   - sym_hash
#   - sym_hash_keys_values
#   - sym_methods
#   - sym_poly
#   - sym_semantics
#   - symbol_upcase_downcase
#   - source_encoding

# === sym_array ===
def t_sym_array
  # Symbol array
  a = [:red, :green, :blue]
  puts a.length
  a.each { |s| puts s }
  
  # [] access
  puts a[0]
  puts a[2]
  
  # Also sym_int_hash each
  h = {x: 10, y: 20}
  h.each do |k, v|
    puts k.to_s + "=" + v.to_s
  end
end
t_sym_array

# === sym_array_empty_push ===
def t_sym_array_empty_push
  # Issue #85 / PR #92: an empty `[]` followed by `push(:sym)` should
  # promote the local's tracked type to sym_array, so element access
  # emits the symbol-name path instead of printing the symbol id.
  # The literal-array form (`syms = [:tag, :more]`) was already passing
  # on master before PR #92, so this test exercises empty-then-push.
  
  syms = []
  syms.push(:tag)
  syms.push(:more)
  puts syms[0]      # tag
  puts syms[1]      # more
  puts syms.length  # 2
  
  # `<<` form should behave the same.
  syms2 = []
  syms2 << :alpha
  syms2 << :beta
  puts syms2[0]     # alpha
  puts syms2[1]     # beta
end
t_sym_array_empty_push

# === sym_array_full ===
def t_sym_array_full
  # Full sym_array test
  a = [:red, :green, :blue, :yellow]
  
  # Basic access
  puts a.length       # 4
  puts a[0]           # red
  puts a[3]           # yellow
  puts a[-1]          # yellow
  
  # each
  a.each { |s| puts s }
  # red green blue yellow
  
  # push
  a.push(:purple)
  puts a.length       # 5
  
  # pop
  v = a.pop
  puts v              # purple
  
  # include?
  puts a.include?(:red)   # true
  puts a.include?(:pink)  # false
  
  # map
  b = a.map { |s| s.to_s }
  puts b.length       # 4
  puts b[0]           # red
  
  # select
  c = a.select { |s| s.to_s.length > 4 }
  puts c.length       # 2
  
  # each_with_index
  a.each_with_index do |s, i|
    if i == 0
      puts s          # red
    end
  end
  
  # sort (by symbol name, lexical)
  d = [:cherry, :apple, :banana]
  d_sorted = d.sort
  puts d_sorted[0]    # apple
  puts d_sorted[1]    # banana
  puts d_sorted[2]    # cherry
  
  # puts of sym_array
  puts a              # should print each element on its own line
end
t_sym_array_full

# === sym_hash ===
def t_sym_hash
  # Symbol-keyed hashes: keys are sp_sym (distinct from string keys).
  # Cover both int-valued (sp_SymIntHash) and string-valued
  # (sp_SymStrHash) shapes — same operator surface, different value
  # types — in one file. Different local names so per-method type
  # inference doesn't unify the two h's.
  
  # Int values
  hi = {a: 1, b: 2, c: 3}
  puts hi[:a]            # 1
  puts hi[:b]            # 2
  puts hi.length         # 3
  puts hi.has_key?(:a)   # true
  puts hi.has_key?(:z)   # false
  puts hi.empty?         # false
  hi[:d] = 4
  puts hi[:d]            # 4
  puts hi.length         # 4
  
  # String values
  hs = {name: "Alice", role: "admin"}
  puts hs[:name]              # Alice
  puts hs[:role]              # admin
  puts hs.length              # 2
  puts hs.has_key?(:name)     # true
  puts hs.has_key?(:unknown)  # false
  hs[:email] = "a@b.c"
  puts hs[:email]             # a@b.c
  puts hs.length              # 3
end
t_sym_hash

# === sym_hash_keys_values ===
def t_sym_hash_keys_values
  # keys / values for sym-keyed hash variants plus str_poly_hash.
  
  # sym_int_hash
  h1 = {a: 1, b: 2, c: 3}
  puts h1.keys.inspect
  puts h1.values.inspect
  puts h1.keys.length
  puts h1.values.length
  
  # sym_str_hash
  h2 = {name: "Alice", role: "admin"}
  puts h2.keys.inspect
  puts h2.values.inspect
  
  # sym_poly_hash (mixed value types)
  h3 = {name: "Alice", age: 30, active: true}
  puts h3.keys.inspect
  puts h3.values.inspect
  
  # str_poly_hash (string keys, mixed values)
  h4 = {"name" => "Alice", "age" => 30, "active" => true}
  puts h4.keys.inspect
  puts h4.values.inspect
  
  h1.keys.each do |k1|
    puts k1
  end
  h2.values.each do |sv|
    puts sv
  end
  h3.keys.each do |k3|
    puts k3
  end
  h3.values.each do |pv|
    puts pv
  end
end
t_sym_hash_keys_values

# === sym_methods ===
def t_sym_methods
  # Symbol methods
  puts :hello.to_s        # hello
  puts :hello.length      # 5
  puts :hello.empty?      # false
  puts (:a == :a)         # true
  puts (:a == :b)         # false
  puts (:a != :b)         # true
  
  # String#to_sym → Symbol
  s = "dynamic".to_sym
  puts s                  # dynamic
  
  # to_sym on literal (compile-time interned)
  t = "hello".to_sym
  puts t                  # hello
  
  # Symbol#to_sym is identity
  puts :foo.to_sym        # foo
  
  # Symbol#<=> (lexical)
  puts (:apple <=> :banana)  # -1
  puts (:banana <=> :apple)  # 1
  puts (:apple <=> :apple)   # 0
  
  # Kernel#p with symbol
  p :hello                # :hello
end
t_sym_methods

# === sym_poly ===
def t_sym_poly
  # Mixed-type array triggers poly_array; symbol elements should
  # go through sp_box_sym / SP_TAG_SYM dispatch.
  arr = [1, "two", :three, 4.0, true]
  arr.each { |v| puts v }
end
t_sym_poly

# === sym_semantics ===
def t_sym_semantics
  # Ruby semantic requirements that Phase 2 satisfies
  
  # 0. Symbol is distinct from String
  puts :a == "a"            # false
  puts "a" == :a            # false
  puts :a != "a"            # true
  
  # 1. Same symbol is equal to itself
  puts :foo == :foo         # true
  
  # 2. Different symbols are not equal
  puts :foo == :bar         # false
  
  # 3. Symbol-keyed hash distinguishes sym keys (Ruby semantics)
  h = {a: 1, b: 2}
  puts h.has_key?(:a)       # true
  puts h.has_key?(:z)       # false
  puts h.has_key?("a")      # false - sym key hash, string arg
  
  # 3b. {a:1}[:a] finds it
  puts h[:a]                # 1
  
  # 4. to_sym idempotent on Symbol
  puts :foo.to_sym == :foo  # true
  
  # 5. String#to_sym + Symbol#to_s round trip
  puts "hello".to_sym.to_s  # hello
  
  # 6. Interned symbol identity ("foo" interns to same ID as :foo)
  puts "foo".to_sym == :foo # true
end
t_sym_semantics

# === symbol_upcase_downcase ===
def t_symbol_upcase_downcase
  # Symbol#upcase and Symbol#downcase return symbols by upper/lower-casing
  # the symbol's name string and re-interning. Mirrors the existing
  # String#upcase / #downcase plumbing — the only delta is `sp_sym_to_s`
  # in front of the case helper and `sp_sym_intern` wrapping the result.
  
  # Symbol#upcase
  puts :hello.upcase
  puts :HELLO.upcase
  puts :MixedCase.upcase
  puts :a.upcase
  puts :_.upcase
  
  # Symbol#downcase
  puts :HELLO.downcase
  puts :hello.downcase
  puts :MixedCase.downcase
  puts :Z.downcase
  
  # Round trip — sym -> upper -> lower returns to original lower form
  puts :foo.upcase.downcase
  puts :BAR.downcase.upcase
  
  # Re-intern stability — equal pre/post-case symbols stay equal
  puts :Hello.upcase == :HELLO
  puts :Hello.downcase == :hello
  puts :a.upcase != :A.downcase
end
t_symbol_upcase_downcase

# === source_encoding ===
def t_source_encoding
  # SourceEncodingNode — the `__ENCODING__` keyword.
  #
  # Spinel sources are assumed to be UTF-8; __ENCODING__ is a small
  # Encoding value whose to_s returns the canonical name.
  
  puts __ENCODING__.to_s
end
t_source_encoding
