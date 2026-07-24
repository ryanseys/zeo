require "psych.so"

module Psych
  # The engine version. Matches the bundled psych gemspec (5.4.0). Gems probe
  # `defined?(Psych::VERSION)` to tell the modern Psych from a bare-bones YAML
  # fallback (e.g. rubygems/yaml_serializer.rb) -- zeo always ships the native
  # engine, so this is always defined (and the fallback, which would define a
  # PARTIAL error hierarchy, is correctly skipped -- hence the full set below).
  VERSION = "5.4.0"

  # CRuby's error hierarchy (oracle-verified against ruby 4.0.5):
  # `Exception < RuntimeError`; `SyntaxError`/`DisallowedClass`/`BadAlias` under
  # it; `AliasesNotEnabled`/`AnchorNotDefined` under `BadAlias`. (CRuby's
  # SyntaxError also carries file/line/column readers; those need the parser to
  # report positions, which this runtime's YAML backend does not surface.)
  class Exception < RuntimeError; end
  class SyntaxError < Exception; end
  class DisallowedClass < Exception; end
  class BadAlias < Exception; end
  class AliasesNotEnabled < BadAlias; end
  class AnchorNotDefined < BadAlias; end
end
