# The lock-ordering probe family: mirrored two-container operations
# (replace, ==, eql?, <=>, string ==, Encoding.compatible?) hammered
# from two threads in OPPOSITE orders. Any builtin that holds both
# containers' locks at once deadlocks here within a few iterations --
# this pinned the five sequential-snapshot/address-order fixes.

a = [1] * 50
b = [2] * 50
s1 = "x" * 50
s2 = "y" * 50
t1 = Thread.new do
  500.times { a.replace(b); a == b; a.eql?(b); a <=> b; s1 == s2; Encoding.compatible?(s1, s2) }
end
t2 = Thread.new do
  500.times { b.replace(a); b == a; b.eql?(a); b <=> a; s2 == s1; Encoding.compatible?(s2, s1) }
end
t1.join
t2.join
puts "no deadlock"
__END__
no deadlock
