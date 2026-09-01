# `defined?` of a qualified path answers by POSITION, exactly like the bare
# name: a constant a later require or a later statement defines is nil here,
# whatever the whole program eventually holds. The runtime registry carries
# every spliced class from startup, so the compiler must fold the nil --
# a probe cannot see "not yet".
p defined?(CSV)
p defined?(CSV::Row)
p defined?(Zed)
p defined?(Zed::Q)
require "csv"
p defined?(CSV)
p defined?(CSV::Row)
module Zed
  class Q; end
end
p defined?(Zed::Q)
