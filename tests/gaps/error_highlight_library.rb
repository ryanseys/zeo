# `error_highlight` is loaded by default in ruby, so `require` answers false
# and the constant is already there. zeo vendors neither.
#
# It is PURE RUBY and its only blocker is one primitive:
# `RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location`, which its
# `prism_find` reaches for exactly because zeo already raises the
# "compiled by prism" RuntimeError that sends it there. See
# `ast_node_id_for_backtrace_location.rb` for the rule (exact, and free at
# run time) and for what actually blocks it (prism's `node_id` is a C field
# the Rust bindings do not expose).
#
# Vendoring it WITHOUT the primitive would be worse than not vendoring it:
# `spot` rescues only around `AbstractSyntaxTree.of`, so every NameError's
# `detailed_message` -- which is now the seam every report renders through --
# would raise instead of describing the error.
#
# The golden was stale: it recorded `true` for the require, where ruby answers
# false because the library is already loaded. Re-oracled.
p require("error_highlight")
p defined?(ErrorHighlight)
p ErrorHighlight.respond_to?(:spot)
