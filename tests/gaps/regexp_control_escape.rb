# A regexp literal reaches the engine with `\cA` already turned into its byte
# by the lexer, so only Regexp.new() with a written-out backslash asks the
# engine to read one -- and it read the letters, so the same pattern meant two
# different things depending on how it was spelled.
# Ported from mruby-regexp 435e4c8fa.
def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

# the two spellings name the same character
t("lit-cA")    { /\cA/ =~ "" }
t("new-cA")    { Regexp.new("\\cA") =~ "" }
t("new-C-A")   { Regexp.new("\\C-A") =~ "" }
t("new-cA-no") { Regexp.new("\\cA") =~ "A" }

# a `\` in the X position opens an escape of its own
t("new-c-esc") { Regexp.new("\\c\\n") =~ "\n" }

# `\C` with anything but `-` after it is the escape ending early
t("short-C")   { Regexp.new("\\CA") =~ "A" }
t("short-c")   { Regexp.new("\\c") =~ "c" }

# `\M-X` sets the high bit, making a byte that starts no character
t("meta")      { Regexp.new("\\M-a") =~ "a" }
t("meta-C")    { Regexp.new("\\M-\\C-a") =~ "a" }

# inside a character class too
t("class-cA")  { Regexp.new("[\\cA]") =~ "" }
