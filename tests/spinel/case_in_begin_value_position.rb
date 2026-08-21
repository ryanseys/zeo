# A `case/in` whose value is the tail of a `begin` block. The matched arm falls
# through there rather than returning, so it ran straight into the no-match
# raise that follows the arms -- every such case/in answered
# NoMatchingPatternError however well it had just matched (#4016). The rescue
# in the report is not the trigger: a bare `begin` does it too, and the pattern
# kind does not matter either.
r = (begin; case 5; in Integer then :m; end; end)
p r

r = (begin; case 5; in Integer then :m; end; rescue => e; e.class; end)
p r

class K; end
r = (begin; case K.new; in K then :m; end; rescue => e; e.class; end)
p r

S = Struct.new(:x)
r = (begin; case S.new(1); in S then :m; end; rescue => e; e.class; end)
p r

D = Data.define(:x)
r = (begin; case D.new(x: 1); in D then :m; end; rescue => e; e.class; end)
p r
r = (begin; case D.new(x: 1); in { x: 1 } then :m; end; rescue => e; e.class; end)
p r

# a member-less Data value, which is what the report reduced to
DML = Data.define
r = (begin; case DML.new; in DML then :matched; end; rescue NoMatchingPatternError => e; e.class; end)
p r

# a genuine no-match still raises, from inside a begin and outside one
r = (begin; case 5; in String then :s; end; rescue => e; e.class; end)
p r

def g(x)
  case x
  in Integer then :i
  end
end
p g(1)
p (g("a") rescue $!.class)

# an else arm keeps winning over the raise
r = (begin; case 5; in String then :s; else :other; end; end)
p r

# and the statement and value forms outside a begin are unchanged
case 5
in Integer then p :stmt
end
p (case 5; in Integer then :val; end)
