# #503. `s.gsub(regex, hash)` on a poly-typed receiver still
# called `sp_re_gsub` (the const-char-* replacement helper) and
# passed the hash pointer where `rep` was expected. C compile
# warned -Wincompatible-pointer-types; binary ran but produced
# runtime garbage (the hash pointer's memory was interpreted as
# a replacement string).
#
# The typed-receiver path (s is concretely `const char *`)
# already routes str_str_hash replacements through
# `sp_re_gsub_str_str_hash` via #485. This test exercises the
# poly path (sp_RbVal lv_s) by passing a poly value into a
# function whose param widens to poly.

class A
  attr_accessor :body
  def initialize; @body = "a&b<c>"; end
end
class B
  attr_accessor :body
  def initialize; @body = 42; end
end

def pick(n)
  n > 0 ? A.new : B.new
end

ESCAPES = { "&" => "&amp;", "<" => "&lt;", ">" => "&gt;" }.freeze
PATTERN = /[&<>]/.freeze

# `s` here is sp_RbVal because the caller passes
# `src.body` which is poly (A#body is String, B#body is Int).
def html_escape(s)
  s.gsub(PATTERN, ESCAPES)
end

src = pick(1)
puts html_escape(src.body)
