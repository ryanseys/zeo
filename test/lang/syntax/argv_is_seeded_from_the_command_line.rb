# No args in this harness: ARGV exists and is empty (CRuby startup
# parity; argv-carrying coverage lives in the conformance corpus).

p ARGV
puts ARGV.length
__END__
[]
0
