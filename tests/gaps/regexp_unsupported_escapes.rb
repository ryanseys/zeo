def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

t("bs-K")   { Regexp.new("a\\Kb") =~ "ab" }
t("bare-p") { Regexp.new("\\p").match("p")[0] }
t("pL")     { Regexp.new("\\pL").match("pL")[0] }
