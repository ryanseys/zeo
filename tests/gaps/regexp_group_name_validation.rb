# A group definition took every byte up to its delimiter as the name, so
# `(?<1>c)` and `(?<a)b>c)` compiled where CRuby raises. The number spelling is
# more than a message difference: it named a group nothing in the pattern could
# reach, every reference syntax reading a leading digit as a number instead.
# Ported from mruby-regexp eef662616.
def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

# a leading digit or '-' is the number spelling, which only a reference may use
t("num-name")       { Regexp.new("(?<1>x)") =~ "x" }
t("dash-lead-name") { Regexp.new("(?<-1>x)") =~ "x" }
t("quote-num")      { Regexp.new("(?'1'x)") =~ "x" }

# a ')' from the second byte on ends the scan before a delimiter
t("paren-in-name")  { Regexp.new("(?<a)b>x)") =~ "x" }
t("ref-bad-paren")  { Regexp.new("(?<a>x)\\k<a)b>") =~ "x" }

# the first byte is exempt in both arms, as it is in CRuby
t("paren-name-def") { Regexp.new("(?<)>x)").match("x")[0] }
t("paren-name-ref") { Regexp.new("(?<)>x)\\k<)>").match("xx")[0] }

# every other byte remains a name character, '-' and spaces included
t("dash-in-name")   { Regexp.new("(?<a-b>x)").match("x")[0] }
t("space-in-name")  { Regexp.new("(?<a b>x)").match("x")[0] }

# the reference arm still reads the number spellings
t("ref-num")        { Regexp.new("(x)\\k<1>").match("xx")[0] }
t("ref-rel")        { Regexp.new("(x)\\k<-1>").match("xx")[0] }

t("names")          { Regexp.new("(?<w>a)(?<z>b)").names }
