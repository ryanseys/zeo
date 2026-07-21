# A ternary whose true arm compiles to a NoMethodError raise (an unresolvable
# call on a poly receiver) and whose false arm is a concrete type must compile:
# the raise arm coerces to the sibling arm's type instead of aborting with a C
# conditional type mismatch. flag is runtime so both arms are emitted; it is
# false here so the string/int arm is taken.
arr = [nil]
u = arr.first
flag = ARGV.length > 5
label = (flag ? u.details : "~")
puts label
other = (flag ? u.missing : 42)
p other
