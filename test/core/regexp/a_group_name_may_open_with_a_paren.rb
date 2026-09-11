# A group name may START with any character, `(` and `)` included, and every
# reference form reaches it. After the first character a `)` ends the name,
# and ruby names the rest of the pattern in its error.
def t(src, subject)
  r = begin
    re = Regexp.new(src)
    m = re.match(subject)
    [m&.to_a, re.names, m&.named_captures].inspect
  rescue => e
    "#{e.class}: #{e.message}"
  end
  puts "#{src.ljust(20)} #{r}"
end

t("(?<)>x)", "x")
t("(?<(>x)", "x")
t("(?<)b>x)", "x")
t("(?')'x)", "x")
t("(?<)>x)\\k<)>", "xx")
t("(?<)>x)\\k<)+0>", "xx")
t("(?<)>x)\\g<)>", "xx")
t("(?<)>x)(?(<)>)y|z)", "xy")
t("(?<a b>x)", "x")
t("(?<a)>x)", "x")
t("(?<a)>x)yz", "x")
t("(?<ab)", "x")
t("\\k<a)>", "x")
t("(?<>x)", "x")
p "x".sub(Regexp.new("(?<)>x)"), "[\\k<)>]")
p Regexp.new("(?<)>x)").match("x")[")"]
__END__
(?<)>x)              [["x", "x"], [")"], {")" => "x"}]
(?<(>x)              [["x", "x"], ["("], {"(" => "x"}]
(?<)b>x)             [["x", "x"], [")b"], {")b" => "x"}]
(?')'x)              [["x", "x"], [")"], {")" => "x"}]
(?<)>x)\k<)>         [["xx", "x"], [")"], {")" => "x"}]
(?<)>x)\k<)+0>       [["xx", "x"], [")"], {")" => "x"}]
(?<)>x)\g<)>         [["xx", "x"], [")"], {")" => "x"}]
(?<)>x)(?(<)>)y|z)   [["xy", "x"], [")"], {")" => "x"}]
(?<a b>x)            [["x", "x"], ["a b"], {"a b" => "x"}]
(?<a)>x)             RegexpError: invalid group name <a)>x)>: /(?<a)>x)/
(?<a)>x)yz           RegexpError: invalid group name <a)>x)yz>: /(?<a)>x)yz/
(?<ab)               RegexpError: invalid group name <ab)>: /(?<ab)/
\k<a)>               RegexpError: invalid group name <a)>>: /\k<a)>/
(?<>x)               RegexpError: group name is empty: /(?<>x)/
"[x]"
"x"
