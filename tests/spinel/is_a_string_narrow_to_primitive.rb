# A `String | nil` value narrowed to String via is_a?(String) must reach a
# string primitive (sp_str_include / start_with? / end_with?) as a real
# const char*, not an sp_RbVal. (#1512)
def has_match?(haystacks, needle)
  return false unless needle.is_a?(String)
  haystacks.any? { |h| h.include?(needle) }
end
p has_match?(["abc", "def"], "bc")   # true
p has_match?(["abc", "def"], nil)    # false

def starts?(s, pre)
  return false unless pre.is_a?(String)
  s.start_with?(pre)
end
p starts?("hello", "he")   # true
p starts?("hello", nil)    # false

def ends?(s, suf)
  return false unless suf.is_a?(String)
  s.end_with?(suf)
end
p ends?("hello", "lo")     # true
p ends?("hello", nil)      # false
