# String#scan with a block sets `$~` (and $1, and pre_match) to the current
# match on each iteration, the same way sub and gsub with a block do.
s = "1a2b3c"
results = []
s.scan(/\d/) { results << $~[0] }
p results
__END__
["1", "2", "3"]
