# CRuby's uncaught-exception report puts the class tag at the end of the
# message's FIRST line and lets the rest run underneath, unindented -- the
# `error_pos`/`print_errinfo` split. zeo appended it after the whole message,
# which nothing noticed while the only multi-line messages came from
# constructs that failed to compile.
#
# A kind collision is exactly that shape: `X is not a class` on one line and
# `<file>:<line>: previous definition of X was here` on the next.
raise ArgumentError, "first\nsecond\nthird"
