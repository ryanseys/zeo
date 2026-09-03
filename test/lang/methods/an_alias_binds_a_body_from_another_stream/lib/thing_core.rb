module Streamed
  class Thing
    attr_reader :v
    def initialize(v) = @v = v
    def ==(other) = other.is_a?(Thing) && v == other.v
    def hash = v.hash
  end
end
