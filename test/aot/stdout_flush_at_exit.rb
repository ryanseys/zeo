# A partial line left in the stdout buffer is flushed by the exit path. The
# linked binary owns its own flush; nothing else writes it.
print "no trailing newline"
__END__
no trailing newline
#@ nonl
