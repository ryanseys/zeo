# Bundled tiny tests (auto-grouped by bundler):
#   - comparable_clamp_string
#   - file_separator_constants
#   - hash_fetch_int_int
#   - i897
#   - i957
#   - i976
#   - i981
#   - kernel_rand_no_arg_float
#   - kernel_string_coercion
#   - nil_coercion
#   - nil_to_h
#   - ruby_version_constants
#   - sym_int_hash_key_value
#   - symbol_name
#   - time_subtraction_returns_float

# === comparable_clamp_string ===
def t_comparable_clamp_string
# Issue #899: Comparable#clamp on a string receiver via strcmp.
puts "b".clamp("a", "c")
puts "z".clamp("a", "c")
puts "a".clamp("b", "c")
end
t_comparable_clamp_string

# === file_separator_constants ===
def t_file_separator_constants
# Issue #891: File::SEPARATOR / PATH_SEPARATOR / ALT_SEPARATOR
# resolved at compile time as string literals.
puts File::SEPARATOR
puts File::PATH_SEPARATOR
puts "a" + File::SEPARATOR + "b"
end
t_file_separator_constants

# === hash_fetch_int_int ===
def t_hash_fetch_int_int
# Hash#fetch on an int->int specialized hash: plain lookup and the 2-arg
# default form (the int->int variant previously had #[] but not #fetch).
h = { 1 => 10, 2 => 20 }
puts h.fetch(1)
puts h.fetch(2)
puts h.fetch(9, -1)
puts h.fetch(0, 42)
puts "done"
end
t_hash_fetch_int_int

# === i897 ===
def t_i897
puts (1..Float::INFINITY).lazy.select { |x| x.odd? }.first(5).inspect
puts (1..Float::INFINITY).lazy.select { |x| x % 3 == 0 }.first(4).inspect
puts (1..Float::INFINITY).lazy.reject { |x| x.even? }.first(5).inspect
puts (1..Float::INFINITY).lazy.first(3).inspect
puts (10..Float::INFINITY).lazy.select { |x| x.even? }.first(3).inspect
puts (1..100).lazy.select { |x| x % 7 == 0 }.first(3).inspect
puts (1..Float::INFINITY).lazy.select { |x| x > 5 }.first
end
t_i897

# === i957 ===
def t_i957
h = {a: 1, b: 2}
p = h.to_proc
puts p.call(:a)
puts p.call(:b)

g = {"x" => 10, "y" => 20}.to_proc
puts g.call("x")
puts g.call("y")
end
t_i957

# === i976 ===
def t_i976
p = Proc.new { |a, b| puts "a=#{a.inspect} b=#{b.inspect}" }
p.call(1)
p.call(1, 2)

q = Proc.new { |x, y, z| puts "x=#{x} y=#{y} z=#{z}" }
q.call(10)
q.call(10, 20)
q.call(10, 20, 30)
end
t_i976

# === i981 ===
def t_i981
puts ("a".."c").to_a.inspect
puts ("A".."D").to_a.inspect
puts ("aa".."ac").to_a.inspect
puts ("az".."bb").to_a.inspect
puts (1..3).to_a.inspect
end
t_i981

# === kernel_rand_no_arg_float ===
def t_kernel_rand_no_arg_float
# Kernel#rand without args returns Float in [0.0, 1.0).
puts rand.class
puts rand(100).class
v = rand
puts v >= 0.0 && v < 1.0
end
t_kernel_rand_no_arg_float

# === kernel_string_coercion ===
def t_kernel_string_coercion
# Issue #879: Kernel#String() coercion routes through .to_s for
# each primitive type. nil -> "". Pre-fix: silently emitted 0.
puts String(42)
puts String(nil).inspect
puts String(true)
puts String("hi")
puts String(:sym)
puts String(3.14)
end
t_kernel_string_coercion

# === nil_coercion ===
def t_nil_coercion
# Issue #871: NilClass coercion methods. v1 covers to_i / to_f /
# to_a. to_c, to_r, to_h deferred (Complex, Rational unsupported;
# typed-Hash empty needs element-type judgement).
puts nil.to_i
puts nil.to_f
puts nil.to_a.inspect
end
t_nil_coercion

# === nil_to_h ===
def t_nil_to_h
# NilClass#to_h returns an empty hash.
p nil.to_h
p nil.to_h.length
p nil.to_h.empty?
end
t_nil_to_h

# === ruby_version_constants ===
def t_ruby_version_constants
# Issue #890: RUBY_VERSION / RUBY_PLATFORM / RUBY_ENGINE built-in
# string constants. Format-only check so the test stays stable
# across platforms — the actual values are runtime-detected.
puts RUBY_VERSION.is_a?(String)
puts RUBY_PLATFORM.is_a?(String)
puts RUBY_ENGINE.is_a?(String)
puts RUBY_PLATFORM.length > 0
end
t_ruby_version_constants

# === sym_int_hash_key_value ===
def t_sym_int_hash_key_value
# Hash#key(value) / #value? / #has_value? on sym_int_hash.
# Missing-key #key returns the empty-sym sentinel since the typed
# sym slot can't carry nil.
h = {a: 1, b: 2, c: 3}
puts h.key(2).inspect
puts h.has_value?(2)
puts h.has_value?(99)
puts h.value?(3)
end
t_sym_int_hash_key_value

# === symbol_name ===
def t_symbol_name
# Symbol#name returns the symbol's name as a string (not a pointer
# value cast as int).
puts :hello.name
puts :foo.name.length
end
t_symbol_name

# === time_subtraction_returns_float ===
def t_time_subtraction_returns_float
# Issue #901: Time - Time returns Float (elapsed seconds). LV
# slot inferred as sp_Time on pass 1, refined to float on pass 2
# once the rhs LV was declared as time. merge_refined_local_type
# now accepts time -> float refinement.
start = Time.now
diff = Time.now - start
puts diff >= 0
puts diff.class
end
t_time_subtraction_returns_float

