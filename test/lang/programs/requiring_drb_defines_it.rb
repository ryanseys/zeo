# The vendored gem loads and `DRb` is defined.
require "drb"
p defined?(DRb)
__END__
"constant"
