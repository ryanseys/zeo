class C
  [:tag].each { |n| attr_accessor(n) }
  if Probe.cpu == "universal"
    ONLY = "universal"
    def which = ONLY
    def self.which = ONLY
  end
end
require "real"
