# io/console's methods answer to reflection, not just to calls -- webrick and
# friends feature-test with `respond_to?(:raw)` before touching the terminal.
require "io/console"
p STDIN.respond_to?(:raw)
__END__
true
