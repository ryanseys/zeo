# frozen_string_literal: true
#
# Synthesized `securerandom` shim, built into zeo (spliced by parse/loader.rs
# when a program `require`s "securerandom"). Real Ruby's securerandom.rb is a
# thin wrapper: the `Random::Formatter` mixin (hex/base64/uuid/random_number/
# ...) sitting on ONE entropy leaf. zeo ships that same shape.
#
# The leaf is `Random.urandom`, which reads the OS CSPRNG (getrandom(2) /
# SecRandomCopyBytes) -- so SecureRandom is genuinely cryptographic, not a
# seeded PRNG. Every formatted method comes from the native `Random::Formatter`
# module, `extend`ed here exactly as the stdlib does.
require 'random/formatter'

module SecureRandom
  VERSION = "0.4.1"

  # The entropy leaf. `Random::Formatter` reaches it through `gen_random`; the
  # public `bytes` alias matches the stdlib's `SecureRandom.bytes`.
  def self.gen_random(n)
    Random.urandom(n)
  end

  def self.bytes(n)
    gen_random(n)
  end
end

# Mix the formatter methods in as SecureRandom's own module methods. The runtime
# call form (not the in-body `extend` directive) routes through the native
# `Random::Formatter` so `SecureRandom.hex` and friends resolve.
SecureRandom.extend(Random::Formatter)
