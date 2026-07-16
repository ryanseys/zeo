# `Array#pack` serializes elements into a byte string by a TEMPLATE of
# directive letters; `String#unpack` reverses it. Each directive optionally
# takes a count (a number, or `*` for "the rest").

# --- integers, with width and endianness -----------------------------------
# C/S/L/Q are 8/16/32/64-bit; lowercase c/s/l/q are the signed forms.
p [65, 66, 67].pack("C*")          # "ABC"  -- one byte each
p [258].pack("S>").bytes           # [1, 2]    -- 16-bit, big-endian (S>)
p [258].pack("v").bytes            # [2, 1]    -- 16-bit, little-endian (v)
p [1].pack("N").bytes              # [0, 0, 0, 1]  -- 32-bit big-endian
p [-1].pack("l").bytes             # [255, 255, 255, 255]  -- signed 32-bit

# ...and back the other way:
p "ABC".unpack("C*")               # [65, 66, 67]
p "\x00\x00\x00\x01".unpack("N")   # [1]
p "\xff\xff\xff\xff".unpack("l")   # [-1]   -- signed
p "\xff\xff\xff\xff".unpack("L")   # [4294967295] -- unsigned

# --- string fields: a (null pad), A (space pad), Z (null-terminated) --------
p ["hi"].pack("a5").bytes          # [104, 105, 0, 0, 0]
p ["hi"].pack("A5").bytes          # [104, 105, 32, 32, 32]
p ["hi"].pack("Z*").bytes          # [104, 105, 0]
p "abc\0\0".unpack("A5")           # ["abc"]  -- A strips trailing NUL/space
p "abc\0de".unpack("Z*")           # ["abc"]  -- Z stops at the first NUL

# --- Base64 (m), hex (H), BER (w), UTF-8 codepoints (U) ---------------------
p ["hello world"].pack("m")        # "aGVsbG8gd29ybGQ=\n"
p "aGVsbG8=\n".unpack("m")          # ["hello"]
p ["ff01"].pack("H*").bytes        # [255, 1]  -- hex pairs -> bytes
p "\xff\x01".unpack("H*")          # ["ff01"]
p [300].pack("w").bytes            # [130, 44] -- BER compressed integer
p "あ".unpack("U*")             # [12354]   -- decode UTF-8 codepoints
p [12354].pack("U")                # "あ"

# --- the result encoding ----------------------------------------------------
# pack yields binary bytes, except an all-`U` template yields UTF-8.
p [65].pack("C").encoding          # #<Encoding:BINARY (ASCII-8BIT)>
p [12354].pack("U").encoding       # #<Encoding:UTF-8>
