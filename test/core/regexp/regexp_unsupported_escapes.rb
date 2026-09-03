def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

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

t("class-R")   { Regexp.new("[\\R]").match("R")[0] }
t("class-K")   { Regexp.new("[\\K]").match("K")[0] }

t("class-p")     { Regexp.new("[\\p{Alpha}]") =~ "a" }
t("class-P-neg") { Regexp.new("[^\\p{Word}\\- \\t]").match("a d_1") }
__END__
bs-R | 0
bs-X | 0
bs-G | 0
bs-G-mid | nil
bs-g-name | "ab"
bs-g-quote | "ab"
prop-p | 0
prop-P | 0
prop-unknown | nil
bare-g | "g"
class-R | "R"
class-K | "K"
class-p | 0
class-P-neg | nil
