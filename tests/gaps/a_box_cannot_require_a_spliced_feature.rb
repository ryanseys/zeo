# A box cannot `require` a feature the compiler SPLICED into main.
#
# Each box gets its own copy of everything it requires -- that is what
# makes `box::Widget != Widget` -- and zeo's run-time load path gives a
# box that for any file it can find on disk. What it cannot give is a
# feature the compiler already resolved and spliced into MAIN: there is no
# file to re-read (the gem lives inside zeo's own tree, not on the machine
# the binary runs on) and no unit of its own to re-run, so the box's
# require raises `LoadError`.
#
# The plan records both ways out and neither is free. Emitting a feature
# unit for every spliced file whenever `hir.boxes > 0` roughly doubles a
# box-using program's code size, so it would have to ride behind
# `--embed-sources`; raising the named `LoadError` is what happens today.
# Its sibling is `a_computed_require_of_a_bundled_gem.rb`, which is the
# same missing tier reached from main rather than from a box.
#
# What DOES work says how narrow this is: a box requiring a file on its
# own `$LOAD_PATH` loads it, privately, and re-executes a file main
# already loaded -- see the second half below.
#
# Oracle: the box gets its own copy of json.
require "json"
b = Ruby::Box.new
begin
  b.require "json"
  p b.eval("JSON.generate([1])")
rescue LoadError => e
  p [:load_error, e.message]
end
