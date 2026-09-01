require "psych.so"
require "psych/nodes"
require "psych/coder"
require "psych/set"
require "psych/omap"
# The tree half rides upstream psych's own pure-Ruby machinery (see
# UPSTREAM.md): Gem::Specification#to_yaml builds its document through
# Psych::Visitors::YAMLTree, and zeo's Emitter writes the tree out.
require "psych/class_loader"
require "psych/scalar_scanner"
require "psych/handler"
require "psych/tree_builder"
require "psych/visitors/visitor"
require "psych/visitors/yaml_tree"
require "psych/visitors/emitter"

module Psych
  # The engine version. Matches the bundled psych gemspec (5.4.0). Gems probe
  # `defined?(Psych::VERSION)` to tell the modern Psych from a bare-bones YAML
  # fallback (e.g. rubygems/yaml_serializer.rb) -- zeo always ships the native
  # engine, so this is always defined (and the fallback, which would define a
  # PARTIAL error hierarchy, is correctly skipped -- hence the full set below).
  VERSION = "5.4.0"

  # CRuby's error hierarchy (oracle-verified against ruby 4.0.5):
  # `Exception < RuntimeError`; `SyntaxError`/`DisallowedClass`/`BadAlias` under
  # it; `AliasesNotEnabled`/`AnchorNotDefined` under `BadAlias`.
  class Exception < RuntimeError; end

  # The six marks CRuby's carries. The native half fills them on the raised
  # object, so there is no `initialize` here to disagree with it -- and a
  # program that rescues one reads `e.line`, which is what rubygems and
  # bundler both report.
  class SyntaxError < Exception
    attr_reader :file, :line, :column, :offset, :problem, :context
  end
  class DisallowedClass < Exception; end
  class BadAlias < Exception; end
  class AliasesNotEnabled < BadAlias; end
  class AnchorNotDefined < BadAlias; end

  # The custom-tag registries `add_tag`/`add_domain_type` fill and the
  # YAMLTree visitor consults. Empty by default, exactly as upstream.
  class << self
    attr_accessor :load_tags, :dump_tags, :domain_types
  end
  self.load_tags = {}
  self.dump_tags = {}
  self.domain_types = {}
end

# A date or timestamp scalar builds a real Date or Time, so the loader
# needs both defined -- CRuby's psych requires `date` for exactly this.
require "date"
require "time" 

# psych's `core_ext.rb`: every object can dump itself. Ruby spells it as
# `psych_to_yaml` with `to_yaml` aliased onto it, so a library that wants the
# unambiguous name can reach past another YAML engine's `to_yaml`.
class Object
  def psych_to_yaml(*options)
    Psych.dump(self, *options)
  end

  alias to_yaml psych_to_yaml
end
