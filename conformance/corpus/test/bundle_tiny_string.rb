# Bundled tiny tests (auto-grouped by bundler):
#   - empty_string_to_sym
#   - str_each_line_chomp
#   - str_lines_chomp
#   - str_tr_empty_replacement
#   - string_ascii_only
#   - string_chomp_separator
#   - string_codepoints
#   - string_delete_multi_arg
#   - string_gsub_empty_pat
#   - string_range_splat
#   - string_scan_capture
#   - string_spaceship
#   - string_squeeze_multi_arg
#   - string_strip_nul
#   - string_symbol_itself
#   - string_tr_s
#   - string_valid_encoding
#   - string_with_nul_byte

# === empty_string_to_sym ===
def t_empty_string_to_sym
# Empty-string interning: "".to_sym and the :"" literal must resolve to the
# same symbol id (an empty symbol once broke the C build).
p("".to_sym == :"")
p(:"" == "".to_sym)
p("abc".to_sym == :abc)
p("".to_sym.to_s == "")
end
t_empty_string_to_sym

# === str_each_line_chomp ===
def t_str_each_line_chomp
# String#each_line(chomp: true) strips trailing line endings.
"a\nb\nc".each_line(chomp: true) { |l| p l }
puts "---"
"a\nb\nc".each_line { |l| p l }
puts "---"
"x\r\ny\r\n".each_line(chomp: true) { |l| p l }
end
t_str_each_line_chomp

# === str_lines_chomp ===
def t_str_lines_chomp
# String#lines(chomp: true) strips line endings from each line.
# Without the chomp: keyword, line endings are preserved.

puts "a\nb\nc".lines(chomp: true).inspect
puts "a\nb\nc".lines.inspect
puts "a\r\nb\r\nc\r\n".lines(chomp: true).inspect
end
t_str_lines_chomp

# === str_tr_empty_replacement ===
def t_str_tr_empty_replacement
# String#tr / tr_s with empty replacement string
# When the to-string is empty, matched characters are deleted.

puts "abc".tr("a", "").inspect
puts "abcdef".tr("a-c", "").inspect
puts "aabbbccc".tr_s("ab", "").inspect
end
t_str_tr_empty_replacement

# === string_ascii_only ===
def t_string_ascii_only
# String#ascii_only? : true iff every byte is 7-bit ASCII.
p "hello".ascii_only?
p "héllo".ascii_only?
p "".ascii_only?
p "abc123!".ascii_only?
p "あ".ascii_only?
end
t_string_ascii_only

# === string_chomp_separator ===
def t_string_chomp_separator
# Issue #881: String#chomp with an explicit separator argument
# strips that suffix. Without the fix the arg was silently dropped
# and the default newline rules were applied.
puts "hello!".chomp("!").inspect
puts "hello\n".chomp.inspect
puts "hello\r\n".chomp.inspect
puts "hello".chomp("!").inspect
puts "hello\n\n".chomp("").inspect
end
t_string_chomp_separator

# === string_codepoints ===
def t_string_codepoints
# Issue #903: String#codepoints returns int_array of UTF-8
# codepoints (not bytes).
puts "hello".codepoints.inspect
puts "あ".codepoints.inspect
puts "".codepoints.inspect
end
t_string_codepoints

# === string_delete_multi_arg ===
def t_string_delete_multi_arg
# String#delete with multiple args deletes the intersection of the
# charsets (each arg is a charset spec).
p "hello".delete("l", "o")
p "hello".delete("lo", "l")
p "hello".delete("l", "h", "e")
p "hello".delete("el", "ello")
p "hello".delete("l")
p "hello".delete("^l")
end
t_string_delete_multi_arg

# === string_gsub_empty_pat ===
def t_string_gsub_empty_pat
# Issue #850: String#gsub with empty pattern inserts the
# replacement between every character (and at the start/end).
# Pre-fix, empty pattern was a no-op.
puts "hello".gsub("", "x")
puts "abc".gsub("", "-")
end
t_string_gsub_empty_pat

# === string_range_splat ===
def t_string_range_splat
# Splatting a string range into an array literal expands it into a
# str_array (the runtime already has sp_StrArray_from_string_range; the
# bug was the literal mis-typing to int_array and emitting a char*-in-
# int-loop).
p [*"a".."e"]
p [*"a"..."d"]        # exclusive range drops the endpoint
p [*"x".."z", "!"]    # splat-first, then a trailing literal
p [*" ".."&"]         # the printable-ASCII idiom from the bug report
end
t_string_range_splat

# === string_scan_capture ===
def t_string_scan_capture
# Issue #880: String#scan returns nested arrays for capture groups.

puts "hello world".scan(/(\w+)/).inspect
puts "a1 b22".scan(/([a-z]+)(\d+)/).inspect
puts "hello world".scan(/\w+/).inspect
puts "ab ac".scan(/a(?:b|c)/).inspect
end
t_string_scan_capture

# === string_spaceship ===
def t_string_spaceship
# Issue #900: String#<=> dispatches strcmp clamped to -1/0/1.
# Pre-fix: always returned 0.
puts "abc" <=> "abd"
puts "abd" <=> "abc"
puts "abc" <=> "abc"
puts "ab" <=> "abc"
end
t_string_spaceship

# === string_squeeze_multi_arg ===
def t_string_squeeze_multi_arg
# String#squeeze with multiple args squeezes runs of chars in the
# intersection of the charsets.
p "aaabbbccc".squeeze("a", "b")
p "aaabbbccc".squeeze("a", "ab")
p "aaabbbccc".squeeze("abc")
p "aaabbbccc".squeeze
end
t_string_squeeze_multi_arg

# === string_strip_nul ===
def t_string_strip_nul
# String#strip / #lstrip / #rstrip remove the NUL byte along with ASCII
# whitespace, matching CRuby.
p "\0 abc \0".strip
p "abc\0\0".rstrip
p "\0\0abc".lstrip
p "  \t\nx\r\f ".strip
p "\0".strip
p "no change".strip
end
t_string_strip_nul

# === string_symbol_itself ===
def t_string_symbol_itself
# Kernel#itself on string and symbol returns self.
puts "hello".itself
puts :foo.itself.inspect
end
t_string_symbol_itself

# === string_tr_s ===
def t_string_tr_s
# Issue #902: String#tr_s translates and squeezes adjacent
# identical translated chars (untranslated runs keep their
# duplicates).
puts "hello".tr_s("l", "r")
puts "aaabbbccc".tr_s("a", "x")
# Without squeeze for comparison
puts "hello".tr("l", "r")
puts "aaabbb".tr_s("ab", "xy")
end
t_string_tr_s

# === string_valid_encoding ===
def t_string_valid_encoding
# String#valid_encoding? — true for ASCII / well-formed UTF-8,
# false for invalid byte sequences.
puts "hello".valid_encoding?
puts "".valid_encoding?
puts "日本語".valid_encoding?
puts "\xff\xff".valid_encoding?
puts "abc\xc3\x28".valid_encoding?
end
t_string_valid_encoding

# === string_with_nul_byte ===
def t_string_with_nul_byte
# Issue #722: a string literal with an embedded NUL byte reports
# the full length, and .bytes iterates all bytes (not strlen-
# truncated at the NUL). Pre-fix the parser AST text format also
# truncated the field at the NUL.
puts "hello\0world".length
puts "hello\0world".bytes.length
puts "a\0b\0c".length
puts "\0\0\0".length
end
t_string_with_nul_byte

