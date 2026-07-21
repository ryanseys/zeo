# String#rindex with position argument
# The position limits the search to codepoint index <= pos.
# Negative pos counts from end: effective = cl + pos.

r = "hello".rindex("l", 2); puts r == nil ? "nil" : r
r = "hello".rindex("l", 3); puts r == nil ? "nil" : r
r = "hello".rindex("l"); puts r == nil ? "nil" : r
r = "hello".rindex("h", 0); puts r == nil ? "nil" : r
r = "hello".rindex("h", 1); puts r == nil ? "nil" : r
r = "hello".rindex("x", 2); puts r == nil ? "nil" : r
r = "hello".rindex("l", 10); puts r == nil ? "nil" : r
# negative position
r = "hello".rindex("l", -1); puts r == nil ? "nil" : r
r = "hello".rindex("l", -2); puts r == nil ? "nil" : r
r = "hello".rindex("l", -3); puts r == nil ? "nil" : r
r = "hello".rindex("l", -4); puts r == nil ? "nil" : r
r = "hello".rindex("l", -6); puts r == nil ? "nil" : r
# empty substring
r = "hello".rindex(""); puts r == nil ? "nil" : r
r = "hello".rindex("", 3); puts r == nil ? "nil" : r
r = "hello".rindex("", 10); puts r == nil ? "nil" : r
r = "hello".rindex("", -1); puts r == nil ? "nil" : r
r = "hello".rindex("", -6); puts r == nil ? "nil" : r
