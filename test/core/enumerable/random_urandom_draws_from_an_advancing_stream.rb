# Random.urandom answers different bytes across fifty calls, and the length
# asked for.
# (spinel issue #2869)
p (0...50).map { Random.urandom(1).bytes[0] }.uniq.size > 1
p Random.urandom(4).bytesize
__END__
true
4
