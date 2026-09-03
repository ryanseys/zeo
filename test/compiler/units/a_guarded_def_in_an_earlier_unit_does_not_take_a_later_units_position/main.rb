module Probe
  def self.cpu = ["arm", "64"].join
end
require "guarded"
puts C.new.which
puts C.which
c = C.new
c.tag = "tagged"
puts c.tag
