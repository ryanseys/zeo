# A mode string is `access[:extenc[:intenc]]`, and everything after the first
# `:` names encodings rather than access.
#
# zeo matched the WHOLE string against its access table, so `File.open(path,
# "r:UTF-8")` raised `ArgumentError: invalid access mode` before it touched the
# filesystem -- and `"r:big5"` holds a `b`, so a scan for the binary flag
# called that one binmode. Bundler reads every gemspec through
# `Gem.open_file(file, "r:UTF-8:-")`, which is how this surfaced.
#
# Three asymmetries here are CRuby's and none is derivable from the others: a
# `b` does NOT force ASCII-8BIT once the tail names an encoding; naming one in
# both the mode string and an `encoding:` keyword is an error rather than a
# precedence rule; and an unknown name in the TAIL warns and falls back where
# the keyword raises.

def t(label)
  print label.ljust(30)
  p(yield)
rescue StandardError => e
  p [e.class, e.message]
end

path = File.join(__dir__, "an_open_mode_string_carries_its_encodings", "sample.txt")

t("r:UTF-8") { File.open(path, "r:UTF-8", &:external_encoding).to_s }
t("r:UTF-8:UTF-16") { File.open(path, "r:UTF-8:UTF-16") { |h| [h.external_encoding.to_s, h.internal_encoding.to_s] } }
t("rb:UTF-8") { File.open(path, "rb:UTF-8") { |h| [h.binmode?, h.external_encoding.to_s] } }
t("rb") { File.open(path, "rb") { |h| [h.binmode?, h.external_encoding.to_s] } }
t("r:bom|utf-8") { File.open(path, "r:bom|utf-8", &:external_encoding).to_s }
t("named twice") { File.open(path, "r:UTF-8", encoding: "Shift_JIS", &:external_encoding).to_s }
t("bad access mode") { File.open(path, "zz") { 1 } }
t("mode: keyword tail") { File.read(path, mode: "r:UTF-8").encoding.to_s }
t("the bytes still read") { File.open(path, "r:UTF-8", &:read) }
