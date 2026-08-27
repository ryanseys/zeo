# A box cannot `require` a feature the compiler already resolved and spliced
# into MAIN: there is no file to re-read (the gem lives inside zeo's own tree,
# not on the machine the binary runs on) and no unit of its own to re-run. It
# is the same decision as `a_computed_require_of_a_bundled_gem.rb`, reached
# from a box rather than from main: `--embed-sources <gems>/json/lib` makes it
# work, and a golden takes no compiler flags. The recorded alternative --
# emitting a feature unit for every spliced file whenever the program uses
# boxes -- roughly doubles a box-using program's code size, so it rides behind
# the same flag.
#
# What DOES work says how narrow this is: a box requiring a file on its own
# `$LOAD_PATH`, or out of the embedded pack, loads it privately
# (`e2e/gems_require.rs::a_box_requires_out_of_the_embedded_pack`).
#
# --- ruby 4.0.6 answers ---
# "[1]"

# A box cannot `require` a feature the compiler SPLICED into main: there is
# no file to re-read and no unit of its own to re-run. A decided divergence
# -- the `.divergence` sidecar records why, and `--embed-sources` is the
# opt-in that makes it work. Oracle: the box gets its own copy of json.
require "json"
b = Ruby::Box.new
begin
  b.require "json"
  p b.eval("JSON.generate([1])")
rescue LoadError => e
  p [:load_error, e.message]
end
