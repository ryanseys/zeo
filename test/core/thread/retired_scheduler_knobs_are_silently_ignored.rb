# `ZEO_THREADS`/`--no-gvl` configure nothing -- threads are always real OS
# threads. A value, even a malformed one, is silently ignored rather than a
# startup error, so existing scripts keep running.
#@ env: ZEO_THREADS=not-a-number
#@ args: --no-gvl

puts 1
__END__
1
