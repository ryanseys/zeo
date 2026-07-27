# bundler (pure Ruby, though large) isn't vendored under gems/ -- `require
# "bundler"` raises LoadError instead of defining the Bundler module. Note
# this is distinct from zeo's own bundler/thor compatibility work on the
# compiler side (see the bundler-northstar memory) -- this is the `bundler`
# gem itself being requirable from ordinary Ruby code.
require "bundler"
p defined?(Bundler)
