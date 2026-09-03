# A STATIC (non-interpolated) invalid pattern is a real, uncatchable
# `SyntaxError` at parse time in real Ruby -- zeo defers that
# check to construction time uniformly (a documented, narrower-timing
# approximation, see `hir::HirNode::RegexpLit`'s docs), so only the
# INTERPOLATED case (genuinely runtime-only in real Ruby too) is
# oracle-verified here as a rescuable exception.

bad = "("
begin
  r = /#{bad}/
  puts "no error"
rescue RegexpError => e
  puts "regexp error"
end
__END__
regexp error
