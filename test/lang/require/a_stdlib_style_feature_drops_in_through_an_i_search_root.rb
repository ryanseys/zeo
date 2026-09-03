# stdlib is delivered as ordinary `-I <lib>` load-path roots (no
# bespoke flag) -- pointing `-I` at a Ruby checkout's `lib` makes each
# `require "feature"` resolve a real stdlib `.rb`. This models that with a
# pure-Ruby "stdlib" file living under an `-I` root, required by name and
# compiled + run through the ordinary loader path.

$LOAD_PATH.unshift(File.expand_path("a_stdlib_style_feature_drops_in_through_an_i_search_root/rubylib", __dir__))
require_relative "a_stdlib_style_feature_drops_in_through_an_i_search_root/main"
__END__
a\ b\ c
