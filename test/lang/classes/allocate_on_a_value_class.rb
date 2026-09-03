# `Class#allocate` on a value class -- the gap this promotes.
#
# Only 2 of zeo's 98 native classes declared a blank value, so `Regexp`,
# `Mutex` and `Queue` all refused where ruby allocates, and `MatchData` gave
# the wrong refusal (`TypeError` for a name ruby `undef`s, which is a
# `NoMethodError`). Every row here now matches ruby.
#
# The blank Regexp is the interesting one: it is UNINITIALIZED rather than
# empty, so `#source` refuses. A blank that quietly answered `""` would be
# a pattern matching everything -- a wrong answer, not a missing one.

def t(l); r=(begin; yield.inspect; rescue Exception=>e; "#{e.class}: #{e.message}"; end); puts format("%-22s %s", l, r); end
t("Regexp.allocate")   { Regexp.allocate.class }
t("Regexp source")     { Regexp.allocate.source }
t("Mutex.allocate")    { Mutex.allocate.class }
t("Queue.allocate")    { Queue.allocate.class }
t("Proc.allocate")     { Proc.allocate.class }
t("MatchData.allocate"){ MatchData.allocate.class }
t("String.allocate")   { String.allocate }
t("Array.allocate")    { Array.allocate }
t("Hash.allocate")     { Hash.allocate }
t("Time.allocate")     { Time.allocate.class }
__END__
Regexp.allocate        Regexp
Regexp source          TypeError: uninitialized Regexp
Mutex.allocate         Thread::Mutex
Queue.allocate         Thread::Queue
Proc.allocate          TypeError: allocator undefined for Proc
MatchData.allocate     NoMethodError: undefined method 'allocate' for class MatchData
String.allocate        ""
Array.allocate         []
Hash.allocate          {}
Time.allocate          Time
