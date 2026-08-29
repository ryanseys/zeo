def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

t("lit-cA")    { /\cA/ =~ "" }
t("new-cA")    { Regexp.new("\\cA") =~ "" }
t("new-C-A")   { Regexp.new("\\C-A") =~ "" }
t("new-cA-no") { Regexp.new("\\cA") =~ "A" }

t("new-c-esc") { Regexp.new("\\c\\n") =~ "\n" }

t("short-C")   { Regexp.new("\\CA") =~ "A" }
t("short-c")   { Regexp.new("\\c") =~ "c" }

t("class-cA")  { Regexp.new("[\\cA]") =~ "" }
