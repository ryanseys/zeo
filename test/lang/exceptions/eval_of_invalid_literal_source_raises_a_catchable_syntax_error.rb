# A literal `eval("...")` whose source doesn't parse does not fail the
# COMPILE: it falls through to the runtime `eval` and raises a catchable
# SyntaxError, exactly as CRuby does.

begin; eval("1 +"); rescue SyntaxError; puts "caught"; end
__END__
caught
