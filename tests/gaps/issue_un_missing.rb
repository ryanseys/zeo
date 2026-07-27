# un is a pure-Ruby bundled utility library (small command-line file-utility
# scripts usable via `ruby -run`, no C extension) that isn't vendored under
# gems/ -- `require "un"` raises LoadError.
require "un"
p true
