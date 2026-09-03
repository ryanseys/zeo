# CRuby registers a feature as loading BEFORE executing it, so the
# inner require of an in-progress file is a no-op and execution order
# is ca-start, all of cb, ca-end -- the dedup-before-lowering rule
# reproduces this exactly.

require_relative "circular_requires_compose_in_rubys_execution_order/main"
__END__
ca start
cb start
cb end
ca end
main done
