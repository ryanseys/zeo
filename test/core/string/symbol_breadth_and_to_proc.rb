# Symbol Tier A + the `&:sym` block-argument conversion
# (`Symbol#to_proc`), previously unsupported.

p :b <=> :a
p :hello.length
p :a.succ
p :HeLLo.downcase
p :he.to_s
p :he.inspect
p :he.upcase
p :he.capitalize
p :he.empty?
p :upcase.to_proc.call("hi")
p [3, 1, 2].map(&:to_s)
p ["b", "a"].map(&:upcase)
__END__
1
5
:b
:hello
"he"
":he"
:HE
:He
false
"HI"
["3", "1", "2"]
["B", "A"]
