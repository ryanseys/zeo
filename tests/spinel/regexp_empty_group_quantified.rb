# Repeating an empty group answers the same empty match however many times it
# runs, so the quantifier applies to an atom that emits no code. The engine
# refused it as "target of repeat operator is not specified", which is the
# error for a quantifier with NO atom -- a distinction the source position
# makes, since the empty group does consume its own bytes.
p("" =~ /(?:)*/)
p("ab" =~ /(?:)+/)
p("ab" =~ /(?:)?/)
p("ab" =~ /a(?:)*b/)
p("ab" =~ /(?:)*?/)
p("ab" =~ /(?:){2}/)
p("ab" =~ /a(?:){0,3}b/)
p "ab".sub(/(?:)*/, "-")
p "abc".scan(/(?:)*/).size

# a quantifier with no atom at all still raises, and so does one after a
# comment group, which consumes source without being an atom
begin
  Regexp.new("*a")
rescue => e
  p e.class
end
begin
  Regexp.new("(?#c)*")
rescue => e
  p e.class
end
