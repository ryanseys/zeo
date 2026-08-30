require "thing_core"
module Streamed
  class Thing
    alias_method :eql?, :==
  end
end
