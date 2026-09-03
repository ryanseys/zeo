# A `require` records itself where it is WRITTEN, not for the whole program
# at once. Every effect of loading a feature -- `$LOADED_FEATURES` naming it,
# a require-gated builtin row becoming answerable -- starts at this line.
#
# zeo satisfies `io/console` and `set` natively and splices `prime` from a
# vendored gem, so all three used to be recorded at startup: a program that
# required one on its last line already answered for it on its first.
# `HirNode::FeatureLoaded` is CRuby's `rb_provide_feature`, emitted at the
# require's own position -- and, like CRuby's, ahead of the file it names.

# Require-gated rows: `respond_to?(:getch)` is how a library decides whether
# the console extension is there at all.
p STDOUT.respond_to?(:winsize)
p IO.instance_methods(false).include?(:getch)
require "io/console"
p STDOUT.respond_to?(:winsize)
p IO.instance_methods(false).include?(:getch)

# A spliced gem is not loaded before its require either.
p $LOADED_FEATURES.any? { |f| f.end_with?("prime.rb") }
require "prime"
p $LOADED_FEATURES.any? { |f| f.end_with?("prime.rb") }

# ...and a second require of one records nothing twice.
require "prime"
p $LOADED_FEATURES.count { |f| f.end_with?("prime.rb") }

# What ruby has loaded before line 1 stays loaded from line 1.
p $LOADED_FEATURES.any? { |f| f.end_with?("set.rb") }
p require("set")
__END__
false
false
true
true
false
true
1
true
false
