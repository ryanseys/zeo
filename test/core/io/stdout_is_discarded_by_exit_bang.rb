# `exit!` is CRuby's `_exit(2)`: no `at_exit` handler, and whatever stdio
# still holds is DISCARDED. So this program prints NOTHING, where the same
# text through `exit` prints all 300 bytes. `std::process::exit` runs
# Rust's own cleanup, which flushes, so the buffer has to be stepped
# around rather than asked politely.
at_exit { $stderr.print "[must not run]" }
print "A" * 300
exit!(0)
__END__
