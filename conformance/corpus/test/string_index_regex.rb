# String#index with a Regexp argument returns the first match position
# (Integer) or nil on no match, like the string-argument form. Previously the
# regex lowered to 0 and fed a bogus arg to the plain-string index path.
p("hello".index(/l/))
p("hello".index(/z/))
p("hello world".index(/o/))
p("hello world".index(/\w+/))
p("hello".index("l"))
p("abcabc".index(/c/))
