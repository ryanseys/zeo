r0 = begin; UncaughtThrowError.new; rescue => e; e.class; end
p r0
r1 = begin; UncaughtThrowError.new(:tag); rescue => e; e.class; end
p r1
p UncaughtThrowError.new(:tag, 5).class
