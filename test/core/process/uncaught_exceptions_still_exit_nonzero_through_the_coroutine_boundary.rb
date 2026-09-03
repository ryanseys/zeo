# The top-level uncaught-raise contract (message on stderr, exit 1) must
# survive the main body's Result crossing back to the OS main thread.

raise "through the boundary"
__END__
#@ stderr
core/process/uncaught_exceptions_still_exit_nonzero_through_the_coroutine_boundary.rb:4:in '<main>': through the boundary (RuntimeError)
#@ exit 1
