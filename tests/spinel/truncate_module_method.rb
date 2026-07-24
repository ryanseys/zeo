# Issue #314: a user-defined module method named `truncate` was
# matched by `infer_method_name_type`'s Float#truncate(n) → Float
# arm purely on the method name, ignoring the receiver. Result: a
# `ViewHelpers.truncate(s, length: 100)` call site (which returns
# a string) inferred as float at the caller, and a poly-arg call
# like `html_escape(truncate(s, length: 100))` boxed the string
# return through `sp_box_float(const char *)` — invalid C.
#
# Surfaced via Roundhouse's emitted real-blog views; minimal repro
# extracted from `app/views/articles/_article.rb`'s
# `io << ViewHelpers.html_escape(ViewHelpers.truncate(article.body,
# length: 100))` line.
#
# Fix: gate the rule on a Float receiver. Float#truncate(n) keeps
# its float typing; module-method `truncate` falls through to the
# user-method-return lookup so it picks up the actual declared
# return type.

module M
  def self.html_escape(s)
    return "" if s.nil?
    s.to_s
  end

  def self.truncate(s, length: 30, omission: "...")
    return "" if s.nil?
    str = s.to_s
    return str if str.length <= length
    cutoff = length - omission.length
    cutoff = 0 if cutoff < 0
    "#{str[0, cutoff]}#{omission}"
  end
end

# Force html_escape to take poly so the call site exercises the
# arg-boxing path that picked sp_box_float pre-fix.
puts M.html_escape(42)                                   # 42
puts M.html_escape("hello")                              # hello
puts M.html_escape(M.truncate("short text", length: 100)) # short text
puts M.html_escape(M.truncate("a very long text indeed", length: 10)) # a very ...

# Float#truncate(n) must still type as float — covers the rule's
# original purpose.
f = 1.234
puts f.truncate(2)                                       # 1.23
puts f.truncate(2) + 0.001                               # 1.2309999999999999
