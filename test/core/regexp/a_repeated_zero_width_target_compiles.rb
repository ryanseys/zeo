# A repeat over a zero-width target -- a lookaround, an anchor, or a group
# holding only one -- compiles and matches. A `+` or `{n}` still runs the
# assertion, and a lazy repeat can take zero turns and leave its capture nil.
[
  ["(?:(?!a))*b?", "b"],
  ["a(?:(?<=a))*b?", "ab"],
  ["\\b*a", "a"],
  ["^*a", "a"],
  ["(?=x)*x", "x"],
  ["(?:(?=(a)))*?a", "a"],
  ["(?:(?=a))+b", "b"],
  ["(?:(?=(a))|(?=(ab)))*b", "b"],
  ["(?:\\b|x)*y", "xy"],
  ["(?<=a\\b)c", "a c"],
  ["x(?=y)+", "xy"],
  ["$+", "a"],
  ["\\A*a", "a"],
  ["(?!b){2}a", "a"],
].each do |src, subject|
  r = begin
    Regexp.new(src).match(subject)&.to_a.inspect
  rescue => e
    "#{e.class}: #{e.message}"
  end
  puts "#{src.ljust(28)} #{subject.inspect.ljust(6)} #{r}"
end
__END__
(?:(?!a))*b?                 "b"    ["b"]
a(?:(?<=a))*b?               "ab"   ["ab"]
\b*a                         "a"    ["a"]
^*a                          "a"    ["a"]
(?=x)*x                      "x"    ["x"]
(?:(?=(a)))*?a               "a"    ["a", nil]
(?:(?=a))+b                  "b"    nil
(?:(?=(a))|(?=(ab)))*b       "b"    ["b", nil, nil]
(?:\b|x)*y                   "xy"   ["xy"]
(?<=a\b)c                    "a c"  nil
x(?=y)+                      "xy"   ["x"]
$+                           "a"    [""]
\A*a                         "a"    ["a"]
(?!b){2}a                    "a"    ["a"]
