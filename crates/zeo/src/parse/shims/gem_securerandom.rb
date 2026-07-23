# frozen_string_literal: true
#
# Synthesized `Gem::SecureRandom` shim, spliced by parse/loader.rs IN PLACE OF
# rubygems'/bundler's vendored copy of the securerandom gem
# (`rubygems/vendor/securerandom/lib/securerandom.rb`, and bundler's identical
# copy). That verbatim copy chooses its entropy source at LOAD time from inside
# `class << self` -- a `begin/rescue/else` that runs `Random.urandom(1)` and
# installs one of two `gen_random` aliases -- a load-time control-flow construct
# zeo's static `class << self` handler can't express.
#
# zeo already ships securerandom natively: the `Random::Formatter` mixin over a
# real `Random.urandom` (getrandom(2)/SecRandomCopyBytes), see
# `shims/securerandom.rb`. So the vendored dup is redirected here and given that
# SAME native implementation under the `Gem::` namespace -- the yaml->psych
# pattern (a require redirected to the library zeo really provides).
require 'random/formatter'

module Gem
  module SecureRandom
    VERSION = "0.4.1"

    # The entropy leaf `Random::Formatter` reaches through `gen_random`.
    def self.gen_random(n)
      Random.urandom(n)
    end

    def self.bytes(n)
      gen_random(n)
    end
  end
end

# Mix the formatter methods (hex/base64/urlsafe_base64/uuid/...) in as
# Gem::SecureRandom's own module methods, exactly as the vendored gem's tail
# `Gem::SecureRandom.extend(Random::Formatter)` does.
Gem::SecureRandom.extend(Random::Formatter)
