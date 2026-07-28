# racc/parser is racc's pure-Ruby generated-parser runtime support (no C
# extension) that isn't vendored under gems/ -- `require "racc/parser"`
# raises LoadError.
require "racc/parser"
p defined?(Racc::Parser)
