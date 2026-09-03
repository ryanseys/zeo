# String#scan with a block is supposed to set `$~` (and $1, $~.pre_match,
# etc) to the current match on each iteration, the same way #sub/#gsub with
# a block do. zeo leaves `$~` nil inside the scan block instead of updating
# it per match.
s = "1a2b3c"
results = []
s.scan(/\d/) { results << $~[0] }
p results
__END__
["1", "2", "3"]
