def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

t("bs-K")      { Regexp.new("a\\Kb") =~ "ab" }
t("bs-R")      { Regexp.new("\\R") =~ "\n" }
t("bs-X")      { Regexp.new("\\X") =~ "a" }

t("bs-G")      { Regexp.new("\\Ga") =~ "a" }
t("bs-G-mid")  { Regexp.new("a\\Gb") =~ "ab" }
t("bs-g-name") { Regexp.new("(?<x>a|b)\\g<x>").match("ab")[0] }
t("bs-g-quote"){ Regexp.new("(?<x>a|b)\\g'x'").match("ab")[0] }
t("prop-p")    { Regexp.new("\\p{Alpha}") =~ "a" }
t("prop-P")    { Regexp.new("\\P{Alpha}") =~ "1" }
t("prop-unknown") { Regexp.new("\\p{Hiragana}") =~ "a" }

t("bare-g")    { Regexp.new("\\g").match("g")[0] }
t("bare-p")    { Regexp.new("\\p").match("p")[0] }
t("pL")        { Regexp.new("\\pL").match("pL")[0] }

t("class-R")   { Regexp.new("[\\R]").match("R")[0] }
t("class-K")   { Regexp.new("[\\K]").match("K")[0] }

t("class-p")     { Regexp.new("[\\p{Alpha}]") =~ "a" }
t("class-P-neg") { Regexp.new("[^\\p{Word}\\- \\t]").match("a d_1") }
