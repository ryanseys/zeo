# `\M-X`, `\C-X` and `\cX` are RUBY'S escapes, not the engine's. `re.c`
# decodes them before the engine ever sees the pattern: `\M-` sets bit 0x80,
# `\C-`/`\c` mask with 0x1f, and the two nest. The pattern's own encoding then
# decides whether the byte they name is a whole character -- which is why
# `\M-a` (0xE1) is "too short" in UTF-8, where it is half a character, and
# `\M-\C-a` (0x81) is "invalid", a byte no UTF-8 character can start with.
#
# `#source` keeps the escape AS WRITTEN whatever the engine was handed.

def t(label)
  r = begin
    yield.inspect
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts format("%-22s %s", label, r)
end

t("M-a") { Regexp.new("\\M-a") =~ "a" }
t("M-C-a") { Regexp.new("\\M-\\C-a") =~ "a" }
t("in extended mode") { Regexp.new("\\M-a", Regexp::EXTENDED) }
t("in a class") { Regexp.new("[\\M-a]") }

t("C-a matches") { Regexp.new("\\C-a") =~ "\x01" }
t("cA matches") { Regexp.new("\\cA") =~ "\x01" }
t("C-a source") { Regexp.new("\\C-a").source.bytes }
t("cA source") { Regexp.new("\\cA").source.bytes }
t("C-a in a class") { Regexp.new("[\\C-a]") =~ "\x01" }

t("M bare") { Regexp.new("\\M") }
t("M- bare") { Regexp.new("\\M-") }
t("c bare") { Regexp.new("\\c") }
t("C bare") { Regexp.new("\\C") }
t("Ca, no dash") { Regexp.new("\\Ca") }
t("duplicate meta") { Regexp.new("\\M-\\M-a") }
t("duplicate control") { Regexp.new("\\C-\\C-a") }

# The inner escape may be any of the ordinary byte escapes.
t("C- of an octal") { Regexp.new("\\C-\\101") =~ "\x01" }
t("C- of a hex") { Regexp.new("\\C-\\x41") =~ "\x01" }
t("C- of a tab") { Regexp.new("\\C-\\t") =~ "\x09" }
t("bad hex") { Regexp.new("\\C-\\xZZ") }
t("unknown inner") { Regexp.new("\\C-\\q") }

# A doubled backslash is a literal backslash, not a prefix.
t("escaped backslash M") { Regexp.new("\\\\M-a") =~ "\\M-a" }
