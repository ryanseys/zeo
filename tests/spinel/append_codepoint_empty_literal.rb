# Five surfaces from the conformance wave, four of which crashed.

# String#<< and #concat take an Integer as a codepoint. It was passed through
# the string slot, so the integer arrived at the concatenation as a pointer.
s = +"abc"
p(s << 100)
p s
t = +"x"
t.concat(233)
p t
u = +"q"
u << 100 << 101
p u
v = +"n"
v.append_as_bytes(100)
p v

# A relational operator against a non-String is Comparable's ArgumentError
# (its <=> answered nil), not a comparison against the operand as a pointer
p(("a" < 1 rescue $!.message))
p(("a" > nil rescue $!.message))
p(("a" >= 3.5 rescue $!.class))
p(("a" <=> 1))
p("a" < "b")

# An empty Hash literal settles at its own variant, which is rarely the
# receiver's: merging it handed one hash struct to another's reader
p({ 1 => 2 }.merge({}))
p({ "a" => 1 }.merge({}))
p({ a: 1 }.merge({}))
p({ 1 => 2 }.merge({ 3 => 4 }))

# print of an empty Array or Hash literal: no container variant claimed it
print([]); print("\n")
print({}); print("\n")
print([1, 2]); print("\n")

# nil never matches: Regexp#match answers nil and match? false, rather than
# walking off a NULL subject
p(/a/.match(nil))
p(/a/.match?(nil))
p(/a/ =~ nil)
x = nil
p(/a/.match(x))
p(/a/ =~ x)
p(/a/.match("a"))
p((/a/ =~ 5 rescue $!.class))
