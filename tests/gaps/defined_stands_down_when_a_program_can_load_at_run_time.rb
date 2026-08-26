# `defined?` folds a name NO compiled file assigns to nil, even in a program
# that can load code at run time -- so a constant a runtime `require` really
# did create reads as undefined while the read beside it answers its value.
#
# The distinction that matters: a file zeo compiled in as a UNIT records its
# constants in `unrun_unit_consts`, and the fold correctly stands down for
# those (`tests/a_unit_toplevel_constant_is_defined_from_a_class_body.rb`
# pins it). A file resolved only at RUN time -- here a `$LOAD_PATH.unshift` of
# a computed directory, so `resolve_require` finds nothing at compile time and
# the call survives to `features::load_from_disk` -- is invisible to that set,
# and `const_form_resolves` then answers `Some(false)` rather than `None`.
#
# The rule this wants is the project's own: a runtime-owned fact must not be
# folded into a compile-time set. `Hir::uses_runtime_eval` is already true for
# exactly the programs that can reach a run-time load, so gating the
# fold-to-false on it is narrow -- a program with no surviving require or eval
# keeps the fold, which is nearly all of them.
#
# The value read is right either way; only `defined?` is wrong, which is what
# makes this a silent one.

$LOAD_PATH.unshift(File.join(__dir__, "defined_stands_down_when_a_program_can_load_at_run_time"))
require "latecomer"

p RUNTIME_MARK
p defined?(RUNTIME_MARK)

class Reader
  def mark = defined?(RUNTIME_MARK)
end
p Reader.new.mark
