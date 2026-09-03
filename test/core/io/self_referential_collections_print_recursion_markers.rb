# Pre-15.2 both of these self-deadlocked on the collection's own
# non-reentrant payload Mutex.
#
# The two rings the program prints are the two it leaves alive.
#@ gccheck: cycle leak: 2 objects (Array x1, Hash x1)

a = [1, 2]
a << a
puts a

h = { k: 1 }
h[:me] = h
puts h
__END__
1
2
[...]
{k: 1, me: {...}}
