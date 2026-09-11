# frozen_string_literal: true
# :markup: markdown

module Prism
  # This module is responsible for converting the prism syntax tree into other
  # syntax trees.
  #
  # zeo: only the Ripper translation is vendored -- it is what `require
  # "ripper"` loads. The `parser` and `ruby_parser` adapters subclass those
  # third-party gems, which zeo does not ship.
  module Translation # steep:ignore
    autoload :Ripper, "prism/translation/ripper"
  end
end
