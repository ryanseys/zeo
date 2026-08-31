class Pureleaf
  def leaf = "leaf"

  def tagged(n)
    "#{leaf}-#{n}"
  end

  def self.kind = :pure
end

module Pureleaf::Deep
  WIDTH = 3
end

PURELEAF_TAG = 7

# An intra-package typed call: `probe` is a known Pureleaf, so the send
# compiles to a typed direct call whose patched-bit guard computes from
# the id-table LOAD -- the variable guard form, exercised at require.
probe = Pureleaf.new
PURELEAF_PROBE = probe.tagged(PURELEAF_TAG)
