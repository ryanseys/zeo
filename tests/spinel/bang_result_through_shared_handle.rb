# A bang mutator on a String read out of a container answers the receiver on a
# change and nil on none. The "did it change?" test compared the receiver's old
# text against the new -- but the old text is the LIVE payload of a shared
# handle, so once the mutation had written the new contents into it the two
# compared equal and the method answered nil after a substitution that plainly
# happened (#4042). The test is made before the mutation now.
h = { a: +"abc" }
s = h[:a]
p s.sub!("b", "*")
p h
p s

h2 = { a: +"abc" }
t = h2[:a]
p t.sub!("z", "*")
p h2

h3 = { a: +"abc" }
u = h3[:a]
p u.gsub!("b", "*")
p u.upcase!
p u.upcase!
p u.swapcase!
p u.reverse!
p h3

a = [+"abc"]
v = a[0]
p v.sub!("b", "*")
p a

# a plain local, which already worked
w = +"abc"
p w.sub!("b", "*")
p w.sub!("z", "*")

# strip!/squeeze!/chomp! answer nil when they change nothing
x = { a: +"  pad  " }[:a]
p x.strip!
p x.strip!
