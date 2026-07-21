# Spinel-allocated strings encode their byte length in the heap
# header (sp_str_hdr.len), but several runtime helpers and one
# codegen `bytesize` emit walked the buffer via NUL-terminated
# strlen/`while(*p)` semantics. A `0.chr` value carrying a header
# length of 1 with byte content `\x00` reported `length=0`,
# `bytesize=0`, and sliced wrong past any embedded NUL.
#
# Fix: `sp_str_length` and `sp_utf8_byte_offset` consume the
# header-tracked byte length instead of terminating at the first
# NUL byte. `bytesize` codegen now emits
# `sp_str_byte_len(...)` instead of `(mrb_int)strlen(...)`.
# Issue #593.

# Single-byte NUL.
puts 0.chr.length              # 1
puts 0.chr.bytesize            # 1
puts 0.chr.ord                 # 0

# Concat preserves NULs in the middle.
b = 0.chr + 0xc8.chr
puts b.length                  # 2
puts b.bytesize                # 2
puts b[0].ord                  # 0
puts b[1].ord                  # 200 (0xc8)

# Mixed ASCII + NUL + ASCII -- indexing past the NUL.
c = "x" + 0.chr + "y"
puts c.length                  # 3
puts c[0].ord                  # 120 ('x')
puts c[1].ord                  # 0
puts c[2].ord                  # 121 ('y')

# Progressive build with NUL in the middle.
d = ""
d = d + 0x81.chr
d = d + 0x7e.chr
d = d + 0x00.chr
d = d + 0xc8.chr
puts d.length                  # 4
puts d[2].ord                  # 0
puts d[3].ord                  # 200

# Substring across NUL.
e = d[0, 4]
puts e.length                  # 4
puts e[3].ord                  # 200
