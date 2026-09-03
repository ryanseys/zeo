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
if defined?(Zed::Q)
  puts "guard above: yes"
else
  puts "guard above: no"
end
if defined?(Wob)
  puts "bare guard above: yes"
else
  puts "bare guard above: no"
end
module Zed
  class Q; end
end
module Wob; end
p defined?(Zed::Q)
if defined?(Zed::Q)
  puts "guard below: yes"
else
  puts "guard below: no"
end
GATED = 1 unless defined?(GATED)
p GATED
__END__
nil
nil
nil
nil
"constant"
"constant"
guard above: no
bare guard above: no
"constant"
guard below: yes
1
