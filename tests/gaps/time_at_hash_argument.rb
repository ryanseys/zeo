# NOT a `Time.at` bug -- this is the keyword-hash-versus-positional-Hash
# distinction, and `Time.at` is only where it happens to surface.
#
# zeo raises `ArgumentError: wrong number of arguments (given 0, expected
# 1..3)`, where ruby raises the TypeError this file asks for. The count is
# the tell: the `{}` never arrived as a positional argument at all.
#
# `Time.at`'s row is `def self."at" allocs (_recv, time, subsec?, unit?,
# **opts)` (crates/zeo-rt/src/builtins/time.rs). The DSL's `**kwrest` peels a
# TRAILING HASH before the argument-count guard runs -- see
# `gen_preamble` in crates/zeo-macros/src/lib.rs -- and it peels ANY trailing
# hash, keyword or not. So `Time.at({})` loses its only argument to `opts`
# and then fails the count.
#
# Ruby keeps the two apart: a hash written as keywords is flagged, and a
# hash passed positionally is an ordinary argument. zeo has no flag on the
# value, which is the same missing model behind
# `tests/gaps/ruby2_keywords_method.rb` and
# `tests/gaps/splat_forward_keeps_hash_positional.rb` -- fixing it here in
# isolation would be a `Time.at` special case, not the fix.
#
# The narrow version, if this one is wanted before the model: make the
# macro's kwrest peel consult `crate::collections::hash_is_kwargs` (which
# already exists and is what `rstruct.rs` uses to tell a keyword-init call
# from a positional Hash) rather than peeling every trailing hash. That
# changes every `**kwrest` row at once, so it needs the full gate.
begin
  Time.at({})
rescue TypeError => e
  p e.class
end
