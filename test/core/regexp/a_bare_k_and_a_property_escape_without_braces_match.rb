# `\K` (keep, an Onigmo extension) and a `\p` property escape written
# without braces. ruby's engine accepts all three: `\K` matches and reports
# offset 0, and a bare `\p` or `\pL` warns "invalid Unicode Property" at
# the caller's line and then matches the letters literally.
#
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
__END__
bs-K | 0
bare-p | "p"
pL | "pL"
#@ stderr
core/regexp/a_bare_k_and_a_property_escape_without_braces_match.rb:16: warning: invalid Unicode Property \p: /\p/
core/regexp/a_bare_k_and_a_property_escape_without_braces_match.rb:17: warning: invalid Unicode Property \p: /\pL/
