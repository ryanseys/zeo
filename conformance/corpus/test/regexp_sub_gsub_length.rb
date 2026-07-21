# Regex String#sub / #gsub build into an over-allocated buffer; the
# result's stored byte length must reflect the bytes actually written,
# not the buffer's slack. Without the fix, #length returned the padded
# capacity and inspect/p showed trailing NUL bytes. Exercise the paths
# that read the metadata length (p / #length), which strlen-based
# operations like puts would otherwise hide.
p "hello".gsub(/l/, "L")
p "hello".gsub(/l/, "L").length
p "hello world".gsub(/o/, "0")
p "hello world".gsub(/o/, "0").length
p "hello".sub(/l/, "L")
p "hello".sub(/l/, "L").length
p "aaa".gsub(/a/, "bb")
p "aaa".gsub(/a/, "bb").length
# Backreference expansion still lands at the right length.
p "FontName Times".gsub(/(\w+) (\w+)/, '\2 \1')
p "abcabc".gsub(/b/, "").length
