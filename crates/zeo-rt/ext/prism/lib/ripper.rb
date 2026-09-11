# frozen_string_literal: true

# zeo's `ripper`: prism's Ripper translation, bound to the `Ripper` name.
#
# CRuby's ripper is a C extension over parse.y's event stream. zeo parses
# with prism, so it answers `require "ripper"` with prism's own translation
# of that interface -- the one prism ships for exactly this purpose.
require "prism"
require "prism/translation/ripper"
require "prism/translation/ripper/shim"
