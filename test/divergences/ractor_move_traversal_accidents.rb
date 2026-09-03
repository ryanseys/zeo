# CRuby's `move:` traversal GUTS objects as it walks, and two of its results
# follow from that rather than from anything the language promises. zeo does not
# copy either.
#
# 1. A REFUSED move has already destroyed the source. `[a, Thread.current]`
#    trips on the Thread, and in CRuby `a` is poisoned by the time the refusal
#    is raised -- the data is gone with nothing delivered. zeo validates the
#    whole graph before it mutates anything, so a refusal leaves every object
#    intact. Copying CRuby means moving before validating.
#
# 2. A DUPLICATED reference in a moved graph husks. `[x, x]` delivers a
#    `Ractor::MovedObject` in the second slot in CRuby, because its replacement
#    table tracks only in-progress ancestors: cycles reconstruct, completed
#    siblings do not. zeo's seen-table preserves the duplicate as one moved
#    object, so `got[0].equal?(got[1])` is true here and false there. Copying
#    CRuby means dropping the seen-table entry once a subtree completes, so a
#    shared object arrives twice with one husk.
#
# Both are cheap to implement and both lose a safety property, which is why zeo
# keeps its answers. The cost is real and stated: a program that RELIES on an
# accident -- checking `MovedError` to detect a failed move, say -- behaves
# differently under zeo, and this file is where that is written down.
#
# --- ruby 4.0.6 answers ---
# [Ractor::Error, "can not move Thread object."]
# [:poisoned, Ractor::MovedError]
# String
# [:husk, Ractor::MovedError]
# false

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
# Resolved 2026-08-22: both are DECISIONS, so this is a passing test whose
# golden records ZEO's output, with the reason and ruby's own answer in the
# `.divergence` sidecar beside it. A program that RELIES on an accident
# (checking `MovedError` to detect a failed move, say) behaves differently
# under zeo, and this file is where that is announced.
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
__END__
[Ractor::Error, "can not move Thread object."]
"survivor"
String
String
true
