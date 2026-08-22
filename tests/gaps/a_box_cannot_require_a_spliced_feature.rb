# A box cannot `require` a feature the compiler SPLICED into main.
#
# Each box gets its own copy of everything it requires -- that is what makes
# `box::Widget != Widget` -- and zeo's run-time load path gives a box that
# for any file it can find on disk, and now for any file in the
# `--embed-sources` pack (`e2e/gems_require.rs::
# a_box_requires_out_of_the_embedded_pack`). What it cannot give is a feature
# the compiler already resolved and spliced into MAIN: there is no file to
# re-read (the gem lives inside zeo's own tree, not on the machine the binary
# runs on) and no unit of its own to re-run.
#
# So this is the same decision as its sibling
# `a_computed_require_of_a_bundled_gem.rb`, reached from a box rather than
# from main: `--embed-sources <gems>/json/lib` makes it work, and a golden
# takes no compiler flags. The alternative the plan records -- emitting a
# feature unit for every spliced file whenever `hir.boxes > 0` -- roughly
# doubles a box-using program's code size, so it would ride behind the same
# flag.
#
# What DOES work says how narrow this is: a box requiring a file on its own
# `$LOAD_PATH`, or out of the pack, loads it privately and re-executes a file
# main already loaded -- see the second half below.
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
