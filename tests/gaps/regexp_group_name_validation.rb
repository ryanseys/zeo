def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

t("paren-name-def") { Regexp.new("(?<)>x)").match("x")[0] }
t("paren-name-ref") { Regexp.new("(?<)>x)\\k<)>").match("xx")[0] }
