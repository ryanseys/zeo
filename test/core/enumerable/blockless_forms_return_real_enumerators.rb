# A blockless map returns a real Enumerator whose `each` re-invokes the
# captured method. Oracle-verified.

e = [1, 2].map
p e.class
p e
p e.each { |x| x * 3 }
p e.size
__END__
Enumerator
#<Enumerator: [1, 2]:map>
[3, 6]
2
