# fsl-true is Spinel's DEFAULT (no pragma in this file): literals are frozen,
# mutation raises, and +"" / String.new / interpolation give mutable strings
# that keep the full shared-mutation semantics.
p "lit".frozen?
s = "m"
begin
  s << "!"
  puts "BUG: no raise"
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
