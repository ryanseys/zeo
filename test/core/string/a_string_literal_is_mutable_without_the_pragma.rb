# With no `frozen_string_literal` pragma, a literal is NOT frozen: `"lit"`
# answers false to frozen? and `<<` on one mutates it.
#
# The rest of the program is the shared-mutation semantics that follow from
# that -- `+""`, String.new and interpolation each give a string two names can
# reach, and a write through one name is seen through the other.
p "lit".frozen?
s = "m"
begin
  s << "!"
  puts "mutated, no raise"
rescue FrozenError => e
  puts e.message
end

m = +"base"
t = m
m << "!"
p t
p m.equal?(t)

n = String.new("n")
arr = [n]
n.upcase!
p arr[0]

who = "world"
i = "hello #{who}"
i << "!"
p i
__END__
false
mutated, no raise
"base!"
true
"N"
"hello world!"
