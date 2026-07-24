# Block-form String#gsub / #sub: the block receives each match and
# returns its replacement. The result used to be padded with trailing
# NUL bytes (the working buffer was returned with its allocation size
# as the byte length, not the content length).
p "abc".gsub(/[ac]/) { |m| m.upcase }
p "hello world".gsub(/o/) { |m| "0" }
p "aaa".gsub(/a/) { |m| m + m }
p "abc".sub(/b/) { |m| m.upcase }
p "x y z".gsub(/\s/) { |m| "_" }
p "no match here".gsub(/z+/) { |m| "!" }

# Replacement much larger than input forces the buffer to grow
# (realloc must operate on a plain malloc'd buffer, not a string alloc).
p ("a" * 100).gsub(/a/) { |m| "XYZ" }
