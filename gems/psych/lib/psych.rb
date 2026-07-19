require "psych.so"

module Psych
  # CRuby: Psych::Exception < RuntimeError, with SyntaxError under it (which in
  # CRuby also carries file/line/column readers; those need the parser to
  # report positions, which this runtime's YAML backend does not surface).
  class Exception < RuntimeError; end
  class SyntaxError < Exception; end
end
