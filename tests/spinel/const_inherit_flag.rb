class Up; TAG = "up"; end
class Down < Up; end
p Down.const_defined?(:TAG)
p Down.const_defined?(:TAG, false)
r = (Down.const_get(:TAG, false) rescue $!.class); p r
p Up.const_defined?(:TAG, false)
p Up.const_get(:TAG, false)
p Down.const_get(:TAG)
