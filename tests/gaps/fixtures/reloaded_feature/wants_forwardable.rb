require "forwardable"
module WantsForwardable
  def self.ok = Forwardable::VERSION.is_a?(String)
end
