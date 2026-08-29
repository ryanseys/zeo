def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

t("meta")      { Regexp.new("\\M-a") =~ "a" }
t("meta-C")    { Regexp.new("\\M-\\C-a") =~ "a" }
