# Character indexing takes a byte-indexed shortcut when every character is one
# byte. Whether that holds is a property of the CONTENT, not just the encoding,
# so it has to be re-decided after anything that changes the bytes.

ascii = "hello world"
p ascii[0], ascii[4], ascii[-1], ascii[11], ascii[3, 5], ascii[-4, 4]

wide = "héllo wörld"
p wide.length, wide[0], wide[1], wide[-1], wide[1, 4], wide[7, 2]

# ASCII first, multibyte appended: the shortcut must stop applying.
grown = "abc"
p grown[1]
grown << "é"
p grown.length, grown[3], grown[1], grown[2, 2]

# Multibyte first, then sliced back down to ASCII.
shrunk = "aébc"
p shrunk[1]
shrunk.replace("abcd")
p shrunk[1], shrunk[3], shrunk[1, 2]

# `setbyte` rewrites bytes underneath the cached answer.
poked = "aaaa".dup
p poked[1]
poked.setbyte(0, 0xC3)
poked.setbyte(1, 0xA9)
p poked.bytesize, poked.length

# Binary strings index by byte whatever the bytes are.
bin = "h\xC3\xA9y".b
p bin.length, bin[1].bytes, bin[1, 2].bytes

# Out-of-range answers on both paths.
p ascii[99], wide[99], ascii[99, 1], wide[99, 1], ascii[-99], ascii[3, -1]

# An empty slice at the very end is "" on both paths, not nil.
p ascii[11, 3], wide[11, 3], "".dup[0, 0]
