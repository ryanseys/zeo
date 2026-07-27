# Widening which regex arguments reach the string-side match path must not
# pull an Object receiver off its own methods: a class that defines match /
# match? answers those itself, whatever the argument expression looks like.
# Kept apart from regex_value_as_match_arg.rb, which needs a program where
# nothing defines #match so a poly receiver stays a String.

class Matcher
  def match(re)
    "user match #{re.source}"
  end

  def match?(re)
    "user match? #{re.source}"
  end
end

m = Matcher.new

# inline call in argument position
p m.match(Regexp.new("\\d"))
p m.match?(Regexp.new("\\d"))

# a local, and a regex literal
lv = Regexp.new("\\w")
p m.match?(lv)
p m.match(/\s/)

# the String receiver in the same program still takes the regex path
p "1d".match(Regexp.new("\\d"))
p "1d".match?(Regexp.new("\\d"))
