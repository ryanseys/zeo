require "set"
p Marshal.load(Marshal.dump(Set[1, 2])).to_a.sort
__END__
[1, 2]
