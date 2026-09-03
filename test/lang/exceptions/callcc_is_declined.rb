# callcc is DECLINED, not pending -- see "Declined" in docs/COMPATIBILITY.md.
# `Continuation#call` re-enters a captured machine-stack snapshot; a
# native-compiled program has no runtime that can restore one, and an
# escape-only callcc would silently break the re-entering callers real users
# of callcc (generators, amb) rely on. CRuby loads the extension (with an
# obsolescence warning, silenced below so the golden is machine-independent);
# zeo answers LoadError. The divergence is stepped around: both sides print
# the same line.
$VERBOSE = nil
begin
  require "continuation"
rescue LoadError
end
puts "callcc: declined (LoadError under zeo; see docs/COMPATIBILITY.md)"
__END__
callcc: declined (LoadError under zeo; see docs/COMPATIBILITY.md)
