# CRuby's move traversal guts objects AS IT WALKS, and zeo deliberately does
# not copy either accident. First: a REFUSED move (`[a, Thread.current]`
# trips on the Thread) has already poisoned `a` in CRuby -- the data is
# destroyed with nothing delivered; zeo validates the whole graph before
# mutating anything, so a refusal leaves every object intact. Second: a
# DUPLICATED reference in a moved graph (`[x, x]`) delivers a
# Ractor::MovedObject husk in the second slot in CRuby (its replacement
# table only tracks in-progress ancestors, so cycles reconstruct but
# completed siblings husk); zeo's seen-table preserves the duplicate as one
# moved object. Both zeo behaviors are strictly safer; both are divergences.
$stderr.reopen(IO::NULL)

r = Ractor.new { Ractor.receive rescue :refused }
a = ["survivor"]
begin
  r.send([a, Thread.current], move: true)
rescue Ractor::Error => e
  p [e.class, e.message]
end
begin
  p a[0]
rescue Ractor::MovedError => e
  p [:poisoned, e.class]
end
r.send(:done)
r.join

r2 = Ractor.new { Ractor.receive }
x = +"dup"
r2.send([x, x], move: true)
got = r2.value
p got[0].class
begin
  p got[1].class
rescue Ractor::MovedError => e
  p [:husk, e.class]
end
p (got[0].equal?(got[1]) rescue :cant)
