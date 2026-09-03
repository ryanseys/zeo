# :deprecated is off by default (prints nothing); :experimental and the
# uncategorized form print to stderr. The kwarg Hash is never itself
# printed.

warn("plain")
warn("dep", category: :deprecated)
warn("exp", category: :experimental)
warn("a", "b", category: :deprecated)
warn("c", "d", category: :experimental)
puts "done"
__END__
done
#@ stderr
plain
exp
c
d
