# Pending milestones

Empty, which is the point: every milestone written so far passes.

A `*.rb` here runs in `Mode::Xfail` with the `.expected` recording **ruby
4.0.6's** answer. The day it starts matching, the suite fails with a promote
message rather than staying quietly green — move the `.rb` and its `.expected`
up one directory.

Put an umbrella entry point here the moment it is worth tracking, with its
measured diagnosis in the file header. See `../README.md`.
