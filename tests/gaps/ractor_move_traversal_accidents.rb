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
#
# What reproducing each would COST, since the header calls them deliberate and
# the next reader will want the price before re-deciding: the first means
# mutating the graph before validating it, so a refusal destroys data; the
# second means dropping the seen-table entry once a subtree completes, so a
# shared object arrives twice with one husk. Both are cheap to implement and
# both lose a safety property.
#
# NOTE the placement rule in this directory's README -- a divergence zeo has
# DECIDED not to reproduce belongs in a passing test that documents it, not
# here. These two have never been through that decision explicitly; they are
# filed as gaps and behave as deliberate. Worth resolving one way or the other
# rather than leaving the file arguing with the rule.
#
# Either way a program that RELIES on an accident (checking `MovedError` to
# detect a failed move, say) behaves differently under zeo without announcing
# it.
#
# The last line is the sharpest: `got[0].equal?(got[1])` is `false` in CRuby
# because the second slot is a husk, and `true` here. Identity through a move
# is preserved by zeo and not by CRuby.
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
