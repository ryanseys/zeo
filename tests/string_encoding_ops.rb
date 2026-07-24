# String operations are CHARACTER-correct and ENCODING-preserving: a UTF-8
# multibyte character counts as one, and a substring/reverse/upcase keeps the
# receiver's encoding instead of silently becoming UTF-8.

# --- UTF-8: multibyte characters count as one ------------------------------
s = "café"
p s.length          # 4   -- characters, not bytes
p s.bytesize        # 5   -- é is two bytes
p s[3]              # "é" -- indexed by CHARACTER
p s[1, 2]           # "af"
p s[1..]            # "afé"
p s.reverse         # "éfac"  -- reversed by character
p s.upcase          # "CAFÉ"  -- full Unicode casing
p "Straße".upcase   # "STRASSE" -- ß expands to SS
p "ÀÉÎ".downcase     # "àéî"
p "résumé".center(10, "·")  # "··résumé··"

# --- BINARY (ASCII-8BIT): every BYTE is a character ------------------------
b = "café".b         # the UTF-8 bytes, retagged binary
p b.length           # 5   -- bytes, not characters
p b[3].bytes         # [195] -- one raw byte...
p b[3].encoding      # #<Encoding:BINARY (ASCII-8BIT)>  -- ...still binary
p b.reverse.bytes    # [169, 195, 102, 97, 99]  -- byte reversal
p b.upcase.bytes     # [67, 65, 70, 195, 169]  -- only ASCII bytes fold

# --- ISO-8859-1 (Latin-1): 1 byte per char, its OWN case map ---------------
latin = "café".encode(Encoding::ISO_8859_1)   # é is one byte (0xE9)
p latin.length            # 4
p latin.upcase.bytes      # [67, 65, 70, 201]  -- é (0xE9) -> É (0xC9)
p latin.upcase.encoding   # #<Encoding:ISO-8859-1>  -- encoding preserved
p latin[3].bytes          # [233]

# --- casing on the same accented text differs by encoding ------------------
# UTF-8 knows é->É; a BINARY copy leaves the é bytes alone.
p "é".upcase                 # "É"
p "é".b.upcase.bytes         # [195, 169]  -- unchanged
