# Regex parity — backreferences and look-around, backed by a backtracking
# engine that transparently kicks in for constructs the fast engine can't do.
# The common fast path is unaffected.

# In-pattern backreferences.
p "hello" =~ /(\w)\1/               # 2 (the doubled "l")
p "abc"   =~ /(\w)\1/               # nil
p "hello world world".scan(/(\w+) \1/)  # [["world"]]
p "book".match?(/(?<c>o)\k<c>/)     # true (named backref)

# Lookahead / negative lookahead.
p "foobar".scan(/foo(?=bar)/)       # ["foo"]
p "foobaz".scan(/foo(?=bar)/)       # []
p "catfish cat".scan(/cat(?!fish)/) # ["cat"]

# Lookbehind / negative lookbehind.
p "$100 €50".gsub(/(?<=\$)\d+/, "#")  # "$# €50"
p "1a2b3".scan(/(?<!\d)[a-z]/)        # ["a", "b"] — letters not preceded by a digit

# Inline comment (ignored).
p "abc".match?(/a(?#skip me)bc/)    # true

# The fast path still handles the everyday cases identically.
m = "2024-01-15".match(/(\d+)-(\d+)-(\d+)/)
p [m[1], m[2], m[3]]                # ["2024", "01", "15"]
p "a,b,,c".split(/,/)               # ["a", "b", "", "c"]
p "Hello".gsub(/l/, "L")            # "HeLLo"
p "aXbXc".scan(/X/).length          # 2
__END__
2
nil
[["world"]]
true
["foo"]
[]
["cat"]
"$# €50"
[]
true
["2024", "01", "15"]
["a", "b", "", "c"]
"HeLLo"
2
