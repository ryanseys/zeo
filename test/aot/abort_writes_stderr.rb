# `abort` writes its message to stderr and exits 1.
puts "out"
abort "stopping here"
__END__
out
#@ stderr
stopping here
#@ exit 1
