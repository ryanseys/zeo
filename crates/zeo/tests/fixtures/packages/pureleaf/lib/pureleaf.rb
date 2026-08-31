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
