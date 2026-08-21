# A successful in-place String mutation returns the receiver. The "did it
# change?" test compared the buffer with the new content AFTER writing it back
# -- and the comparison operand was sp_String_cstr, a pointer INTO the buffer
# rather than a snapshot of it -- so every successful mutation compared equal
# and answered nil (#4014). Printing the two separately hid it.
a = +"abc"
v = a.sub!("b", "*")
p [a, v]

a = +"abc"
v = a.gsub!("b", "*")
p [a, v]

a = +"abc"
v = a.upcase!
p [a, v]

a = +"abc"
v = a.sub!("b", "*")
p [v, a]

# a mutation that changes nothing still answers nil
a = +"abc"
p [a, a.sub!("z", "*")]
a = +"ABC"
p [a, a.upcase!]
a = +"abc"
p [a.downcase!, a.downcase!]
a = +" x "
p [a.strip!, a.strip!]

# and the non-nil-on-no-change mutators answer the receiver either way
a = +"abc"
p [a.reverse!, a]
