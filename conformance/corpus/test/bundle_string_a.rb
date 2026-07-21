# Bundled tests:
#   - str_array_each
#   - str_array_map
#   - str_array_range
#   - str_format_arr
#   - str_justify
#   - strindex
#   - string_chop
#   - string_each_byte
#   - string_escape
#   - string_format_single

# === str_array_each ===
def t_str_array_each
  # Array#each on a str_array. The block param must shadow an outer
  # same-named mrb_int local. Body skips `puts i` to keep this scope-
  # shadow case separate from the format-specifier path.
  
  3.times do |i|
    puts i
  end
  foo = ["a", "b", "c"]
  n = 0
  foo.each { |i| n = n + 1 }
  puts n   # 3
end
t_str_array_each

# === str_array_map ===
def t_str_array_map
  # Array#map on a str_array. Issue #43: compile_map_expr had no
  # str_array branch, so `tt = foo.map { |s| ... }` silently emitted
  # `lv_tt = 0` and the subsequent iteration crashed.
  
  # String -> string map (the original failure mode)
  foo = ["a", "b", "c"]
  tt = foo.map { |s| s.upcase }
  tt.each { |t| puts t }   # A B C
  
  # String -> int map: codepoint length, returning int_array
  sizes = foo.map { |s| s.length }
  puts sizes.length         # 3
  puts sizes[0]             # 1
  puts sizes[1]             # 1
  puts sizes[2]             # 1
  
  # Original receiver is unchanged
  puts foo[0]               # a
  puts foo[1]               # b
  puts foo[2]               # c
  
  # .map { ... }.each { ... } chain (the issue's pattern)
  words = ["alpha", "beta", "gamma"]
  words.map { |w| w.upcase }.each { |w| puts w }   # ALPHA BETA GAMMA
  
  # Empty input
  empty = "".split(",")
  e2 = empty.map { |s| s.upcase }
  puts e2.length            # 0
  
  # Block parameter is block-local: reusing the same name as an outer
  # differently-typed local must not leak. Issue #43 originally hit this
  # (3.times do |i| ... end then foo.map {|i| ...} where the times-block
  # had typed lv_i as mrb_int).
  rs = []
  3.times do |i|
    rs << "row#{i}"
  end
  out = rs.map { |i| i.upcase }
  out.each { |line| puts line }   # ROW0 ROW1 ROW2
end
t_str_array_map

# === str_array_range ===
def t_str_array_range
  # StrArray slicing: a[range] and a[start, len].
  # Same regression class as IntArray: a[1..2] failed to compile and
  # a[1, 2] silently dropped the second arg.
  
  a = "alpha,beta,gamma,delta,epsilon".split(",")
  
  # Range form
  b = a[1..3]
  puts b.length      # 3
  puts b[0]          # beta
  puts b[1]          # gamma
  puts b[2]          # delta
  
  # (start, len) form
  c = a[1, 2]
  puts c.length      # 2
  puts c[0]          # beta
  puts c[1]          # gamma
  
  # Negative start
  d = a[-2, 2]
  puts d.length      # 2
  puts d[0]          # delta
  puts d[1]          # epsilon
  
  # len exceeds remaining: clamped
  f = a[2, 100]
  puts f.length      # 3
  puts f[0]          # gamma
  puts f[2]          # epsilon
  
  # Bare a[i] still returns a string
  puts a[0]          # alpha
  puts a[-1]         # epsilon
  
  # Result is usable as a StrArray
  puts a[1..3].join(":")  # beta:gamma:delta
end
t_str_array_range

# === str_format_arr ===
def t_str_format_arr
  # String#% with a str_array RHS, routed through sp_str_format_strarr.
  # The integer % path (sp_imod) is unchanged — only str_array RHS triggers
  # sprintf-style formatting.
  
  # Single placeholder
  puts "hello, %s!" % ["world"]            # hello, world!
  
  # Multiple placeholders
  puts "%s and %s" % ["a", "b"]            # a and b
  
  # %% literal
  puts "100%% of %s" % ["coverage"]        # 100% of coverage
  
  # %s after literal text
  puts "[%s][%s][%s]" % ["x", "y", "z"]    # [x][y][z]
  
  # Extra args are ignored (matches CRuby tolerant behavior)
  puts "%s" % ["a", "b", "c"]              # a
  
  # Format string with no placeholders, args ignored
  puts "no formatting here" % ["x"]        # no formatting here
end
t_str_format_arr

# === str_justify ===
def t_str_justify
  # Test String#ljust, rjust, center with custom pad string
  
  # center with default (space) pad
  puts "hi".center(10)        #     hi
  puts "hi".center(9)         #    hi
  
  # center with single-char pad
  puts "hi".center(10, "-")   # ----hi----
  puts "hi".center(9,  "-")   # ---hi----
  
  # center with multi-char pad (cycling)
  puts "hi".center(10, "ab")  # ababhiabab
  puts "hi".center(9,  "ab")  # abahiabab
  
  # ljust with multi-char pad (cycling)
  puts "hi".ljust(8, "ab")    # hiababab
  puts "hi".ljust(8, "xyz")   # hixyzxyz
  
  # rjust with multi-char pad (cycling)
  puts "hi".rjust(8, "ab")    # abababhi
  puts "hi".rjust(8, "xyz")   # xyzxyzhi
  
  # no-op when string already long enough
  puts "hello".ljust(3, "x")  # hello
  puts "hello".rjust(3, "x")  # hello
  puts "hello".center(3, "x") # hello
  
  puts "done"
end
t_str_justify

# === strindex ===
def t_strindex
  s = "hello world"
  puts s[0]    # h
  puts s[6]    # w
  puts s[-1]   # d
  puts s[4]    # o
  puts "done"
end
t_strindex

# === string_chop ===
def t_string_chop
  # basic
  puts "hello".chop
  
  # crlf removed together
  puts "hello\r\n".chop
  
  # newline only
  puts "hello\n".chop
  
  # empty string
  puts "".chop
  
  # single char
  puts "x".chop
  
  # already no trailing newline
  puts "abc".chop
end
t_string_chop

# === string_each_byte ===
def t_string_each_byte
  # String#each_byte iterates the bytes of a string. Mirrors each_char
  # but yields the (unsigned) byte value at each position rather than
  # a single-char substring. ASCII-only test pins parity with CRuby's
  # byte-level iteration. Multi-byte UTF-8 prints the underlying byte
  # values, not codepoints (matches CRuby).
  
  # ASCII string
  "ab".each_byte { |b| puts b }
  
  # Empty string yields nothing
  "".each_byte { |b| puts b }
  
  # Mixed alphabetic and digit
  "A1z".each_byte { |b| puts b }
  
  # Newline byte
  "a\n".each_byte { |b| puts b }
  
  # Multi-byte UTF-8 (Latin Small Letter E with Acute): byte iteration, not codepoint
  "é".each_byte { |b| puts b }
  
  # Counted iteration via accumulator
  total = 0
  "hello".each_byte { |b| total = total + b }
  puts total
  
  # String#each_byte returns the receiver (CRuby parity). Pre-fix Spinel's
  # each_byte was statement-only and the assignment dropped the value.
  total2 = 0
  ret = "hello".each_byte { |b| total2 = total2 + b }
  puts total2
  puts ret
end
t_string_each_byte

# === string_escape ===
def t_string_escape
  # Literal backslash preservation — the original bug: "\\n" is two
  # bytes (backslash + n) in Ruby source-level escape, and length is 2.
  puts "\\n".length     # 2
  puts "a\\nb".length   # 4
  puts "\\\\".length    # 2
  
  # Standard runtime escape sequences must still work alongside the
  # fix — "\n" is one newline byte, "\t" tab, "\r" CR, "\"" quote.
  puts "\n".length      # 1
  puts "\t".length      # 1
  puts "\r".length      # 1
  puts "\"".length      # 1
  puts "a\nb".length    # 3
  
  # Mixed escapes — literal backslash AND a real newline in the same
  # string. "a\\nb\nc" = a, \, n, b, <newline>, c → length 6.
  puts "a\\nb\nc".length
end
t_string_escape

# === string_format_single ===
def t_string_format_single
  # `"fmt" % value` (the single-value form of String#%) used to fall
  # through to sp_imod, which rejected the format string as a non-int.
  
  puts "%d" % 42
  puts "%05d" % 7
  puts "%.2f" % 3.14159
  puts "%s!" % "hello"
end
t_string_format_single

