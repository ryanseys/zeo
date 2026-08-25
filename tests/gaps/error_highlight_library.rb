# `error_highlight` is loaded by default in ruby, so `require` answers false
# and the constant is already there. zeo vendors neither.
#
# It is PURE RUBY and its one blocker is GONE. The primitive it reaches for,
# `RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location`, landed
# 2026-08-24 -- see `tests/ast_node_id_for_backtrace_location.rb` for the
# rule and the six shapes it is verified over. What remains is the vendoring
# itself: 1,102 lines plus its `detailed_message` hook.
#
# The safety valve the primitive ships with is what makes vendoring safe.
# `spot` rescues `ArgumentError`, which is exactly what the primitive raises
# when a location carries no callee, so a shape the rule misses degrades to a
# plain message instead of breaking every `detailed_message` -- the seam
# every report now renders through.
#
# THE FIRST LINE IS NOT WHAT IT LOOKS LIKE, corrected 2026-08-21. A plain
# `ruby` answers `false` to the require, because error_highlight is loaded by
# default. The GOLDEN HARNESS is not a plain ruby: it runs the oracle with
# `--disable-error_highlight --disable-did_you_mean`, so the library is NOT
# loaded there and the require answers `true`. An earlier pass recorded the
# plain-ruby value by hand and called it re-oracled; `cargo xtask bless
# gap::` put the harness's own answer back.
#
# So this file cannot demonstrate "loaded by default" at all -- the two lines
# below it are the real gap: zeo has no `ErrorHighlight` to find. The
# default-loaded behaviour is a `-e` question, and it belongs with the
# vendoring work rather than here.
#
# The lesson is the one `docs/GEM_TESTING.md` already records for gemtests: a
# golden's oracle is the harness's invocation, not the one you type.
p require("error_highlight")
p defined?(ErrorHighlight)
p ErrorHighlight.respond_to?(:spot)
