# Array#pack("U*") encodes integer codepoints as UTF-8. The runtime
# `U` directive was missing, so pack returned an empty string.
p [0x1E0A].pack("U*").bytes       # E1 B8 8A
p [0x3042].pack("U*").bytes       # E3 81 82  (HIRAGANA A)
p [104, 105].pack("U*")           # "hi" (ASCII, 1 byte each)
p [0x1F600].pack("U*").bytes      # F0 9F 98 80 (4-byte emoji)
p [65, 0x3042, 66].pack("U*").bytes
p [0x7F].pack("U*").bytes         # boundary: last 1-byte
p [0x80].pack("U*").bytes         # boundary: first 2-byte
p [0x800].pack("U*").bytes        # boundary: first 3-byte
p [].pack("U*")                   # empty array -> ""
p [97].pack("U")                  # single, no count
