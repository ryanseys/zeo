# `error_highlight` is loaded by default in ruby, so `require` answers false
# and the constant is already there. zeo vendors neither.
#
# It is PURE RUBY and its only blocker is one primitive:
# `RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location`, which its
# `prism_find` reaches for exactly because zeo already raises the
# "compiled by prism" RuntimeError that sends it there. See
# `ast_node_id_for_backtrace_location.rb` for the rule (exact, and free at
# run time) and for what actually blocks it -- the CALLEE, which a
# `Thread::Backtrace::Location` does not carry. (Not prism's `node_id`: that
# field is reachable through the `Node` enum's variants.)
#
# Vendoring it WITHOUT the primitive would be worse than not vendoring it:
# `spot` rescues only around `AbstractSyntaxTree.of`, so every NameError's
# `detailed_message` -- which is now the seam every report renders through --
# would raise instead of describing the error.
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
