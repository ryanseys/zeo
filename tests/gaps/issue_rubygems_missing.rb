# rubygems is CRuby's package-manager library (pure Ruby, though large) --
# it isn't vendored under gems/ at all, so `require "rubygems"` raises
# LoadError instead of defining the Gem module.
require "rubygems"
p defined?(Gem)
