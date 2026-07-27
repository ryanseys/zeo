# irb (pure Ruby, though large) isn't vendored under gems/ -- `require
# "irb"` raises LoadError instead of defining the IRB module.
require "irb"
p defined?(IRB)
