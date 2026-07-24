# String#[]= across every index shape.

# Single integer index: replace one char, negative index, append at length,
# and multi-char replacement.
s = "hello"; s[0] = "H"; p s
s = "hello"; s[-1] = "O"; p s
s = "ab";    s[2] = "c"; p s
s = "abc";   s[1] = "XYZ"; p s

# (start, length): replace a run, insert (length 0), and clamp past the end.
s = "hello"; s[0, 2] = "XY"; p s
s = "hello"; s[1, 0] = "__"; p s
s = "hello"; s[1, 10] = "Z"; p s
s = "hello"; s[-2, 1] = "X"; p s

# Range: inclusive, exclusive, endless, beginless, and an empty range (insert).
s = "hello"; s[1..3] = "__"; p s
s = "hello"; s[1...3] = "Z"; p s
s = "hello"; s[3..] = "END"; p s
s = "hello"; s[..1] = "HE"; p s
s = "hello"; s[2..1] = "X"; p s

# Substring and Regexp (whole match and a capture group).
s = "hello"; s["ll"] = "LL"; p s
s = "hello"; s[/l+/] = "L"; p s
s = "hello world"; s[/(\w+) (\w+)/, 2] = "RUBY"; p s

# The assignment expression evaluates to the RHS.
s = "hello"; p(s[0, 2] = "XY")

# Error paths: out-of-range index/length, unmatched substring/regexp, bad range.
s = "hi"; begin; s[3] = "x"; rescue IndexError => e; puts e.message; end
s = "hi"; begin; s[-3] = "x"; rescue IndexError => e; puts e.message; end
s = "hello"; begin; s[1, -1] = "x"; rescue IndexError => e; puts e.message; end
s = "hello"; begin; s["zz"] = "x"; rescue IndexError => e; puts e.message; end
s = "hello"; begin; s[/z/] = "x"; rescue IndexError => e; puts e.message; end
s = "hello"; begin; s[10..] = "x"; rescue RangeError => e; puts "#{e.class}: #{e.message}"; end
