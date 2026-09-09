# A group name containing `)` -- `(?<)>x)` -- and a backreference to it,
# `\k<)>`. ruby's parser reads the name to the matching `>` and accepts
# both; Oniguruma rejects the name, so zeo raises where ruby matches.
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
__END__
paren-name-def | "x"
paren-name-ref | "xx"
