# `require "find"` -- the vendored gem loads and `Find.find` answers.
require "find"
p Find.respond_to?(:find)
