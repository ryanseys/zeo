# frozen_string_literal: true
# :markup: markdown

# zeo: pull in the statically linked native half FIRST, so `Prism::Zeo` exists
# for `prism/zeo.rb` to call. CRuby's loader idiom -- see
# `gems/strscan/lib/strscan.rb` for the same shape and the reason for it.
require "prism.so"

# The Prism Ruby parser.
#
# "Parsing Ruby is suddenly manageable!"
#   - You, hopefully
#
module Prism
  # There are many files in prism that are templated to handle every node type,
  # which means the files can end up being quite large. We autoload them to make
  # our require speed faster since consuming libraries are unlikely to use all
  # of these features.

  autoload :BasicVisitor, "prism/visitor"
  autoload :Compiler, "prism/compiler"
  autoload :DesugarCompiler, "prism/desugar_compiler"
  autoload :Dispatcher, "prism/dispatcher"
  autoload :DotVisitor, "prism/dot_visitor"
  autoload :DSL, "prism/dsl"
  autoload :InspectVisitor, "prism/inspect_visitor"
  autoload :LexCompat, "prism/lex_compat"
  autoload :MutationCompiler, "prism/mutation_compiler"
  autoload :Pack, "prism/pack"
  autoload :Pattern, "prism/pattern"
  autoload :Reflection, "prism/reflection"
  autoload :Relocation, "prism/relocation"
  autoload :Serialize, "prism/serialize"
  autoload :StringQuery, "prism/string_query"
  # zeo: `Prism::Translation` (the `parser`- and `ripper`-gem adapters) is not
  # vendored -- it subclasses those third-party gems, which zeo does not ship.
  autoload :Visitor, "prism/visitor"

  # Some of these constants are not meant to be exposed, so marking them as
  # private here.

  private_constant :LexCompat

  # Raised when requested to parse as the currently running Ruby version but Prism has no support for it.
  class CurrentVersionError < ArgumentError
    # Initialize a new exception for the given ruby version string.
    def initialize(version)
      message = +"invalid version: Requested to parse as `version: 'current'`; "
      segments =
        if version.match?(/\A\d+\.\d+.\d+\z/)
          version.split(".").map(&:to_i)
        end

      if segments && ((segments[0] < 3) || (segments[0] == 3 && segments[1] < 3))
        message << " #{version} is below the minimum supported syntax."
      else
        message << " #{version} is unknown. Please update the `prism` gem."
      end

      super(message)
    end
  end

  # :call-seq:
  #   Prism::lex_compat(source, **options) -> LexCompat::Result
  #
  # Returns a parse result whose value is an array of tokens that closely
  # resembles the return value of Ripper::lex.
  #
  # For supported options, see Prism::parse.
  def self.lex_compat(source, **options)
    LexCompat.new(source, **options).result # steep:ignore
  end

  # :call-seq:
  #   Prism::load(source, serialized, freeze) -> ParseResult
  #
  # Load the serialized AST using the source as a reference into a tree.
  def self.load(source, serialized, freeze = false)
    Serialize.load_parse(source, serialized, freeze)
  end
end

require_relative "prism/polyfill/byteindex"
require_relative "prism/polyfill/warn"
require_relative "prism/node"
require_relative "prism/node_ext"
require_relative "prism/parse_result"

# zeo: upstream picks between the C extension (CRuby) and the FFI backend
# (every other engine) here. zeo links the same prism C library its own front
# end parses with, and reaches it through a built-in module rather than
# `dlopen` -- so it takes a third branch, structured exactly like the FFI one:
# the native surface is the handful of `pm_serialize_*` entry points, and
# `Prism::Serialize` decodes their buffers in Ruby.
Prism::BACKEND = :ZEO

require_relative "prism/zeo"
