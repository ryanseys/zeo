# UncaughtThrowError.new raises with no arguments and with one, and answers with two.
# (spinel issue #3088)
r0 = begin; UncaughtThrowError.new; rescue => e; e.class; end
p r0
r1 = begin; UncaughtThrowError.new(:tag); rescue => e; e.class; end
p r1
p UncaughtThrowError.new(:tag, 5).class
__END__
ArgumentError
ArgumentError
UncaughtThrowError
