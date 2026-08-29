def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

t("num-name")       { Regexp.new("(?<1>x)") =~ "x" }
t("dash-lead-name") { Regexp.new("(?<-1>x)") =~ "x" }
t("quote-num")      { Regexp.new("(?'1'x)") =~ "x" }

t("paren-in-name")  { Regexp.new("(?<a)b>x)") =~ "x" }
t("ref-bad-paren")  { Regexp.new("(?<a>x)\\k<a)b>") =~ "x" }

t("dash-in-name")   { Regexp.new("(?<a-b>x)").match("x")[0] }
t("space-in-name")  { Regexp.new("(?<a b>x)").match("x")[0] }

t("ref-num")        { Regexp.new("(x)\\k<1>").match("xx")[0] }
t("ref-rel")        { Regexp.new("(x)\\k<-1>").match("xx")[0] }

t("names")          { Regexp.new("(?<w>a)(?<z>b)").names }
