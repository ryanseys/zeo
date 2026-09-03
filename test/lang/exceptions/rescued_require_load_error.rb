# A require inside `begin ... rescue LoadError` whose FILE RESOLVES but whose
# top level RAISES LoadError when it runs -- power_assert's TracePoint probe,
# which test-unit requires exactly this way and degrades by rescuing. The
# spliced statements sit inside a synthesized begin carrying the written
# rescue clauses, so the raise lands in the handler CRuby's require timing
# would give it, and execution continues after the begin.
begin
  require_relative "rescued_require_load_error/probe"
rescue LoadError => e
  p e.message
end
p :after
__END__
"probe says no"
:after
