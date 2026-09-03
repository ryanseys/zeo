# `require "English"` -- the vendored gem is nothing but `alias $NAME $special`
# lines, so it exercises the whole special-global surface: an alias resolves to
# a NAME, and the name is what says whether the value lives in the `$foo` table
# or somewhere else (the match slot, the exception being handled, the last
# child's status).
require "English"

"hello world" =~ /(o w)o/
p [$MATCH, $PREMATCH, $POSTMATCH, $LAST_PAREN_MATCH]
p $LAST_MATCH_INFO[0]
p $PID == Process.pid

# An alias is defined from the moment it is created; its target still follows
# whether a match participated.
p [defined?($MATCH), defined?($ERROR_INFO), defined?($CHILD_STATUS)]

def attempt
  yield
rescue Exception => e
  "#{e.class}: #{e.message}"
end

# Assigning a read-only special reports the spelling the program wrote.
p attempt { $MATCH = "x" }
p attempt { $! = RuntimeError.new("z") }
p attempt { $@ = ["a"] }

# `$~` is the one assignable special, and clearing it clears everything derived.
$LAST_MATCH_INFO = nil
p [$MATCH, $~]

begin
  raise "boom"
rescue
  p [$ERROR_INFO.message, $ERROR_POSITION.class]
end
p $ERROR_POSITION
__END__
["o wo", "hell", "rld", "o w"]
"o wo"
true
["global-variable", "global-variable", "global-variable"]
"NameError: $MATCH is a read-only variable"
"NameError: $! is a read-only variable"
"ArgumentError: $! not set"
[nil, nil]
["boom", Array]
nil
