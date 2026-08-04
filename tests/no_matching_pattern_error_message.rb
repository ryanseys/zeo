# `NoMatchingPatternError` names the value and the test that failed:
#
#     1: String === 1 does not return true
#
# zeo raises the same class with the message "no matching pattern", which says
# only that something did not match.
#
# The detail is the whole diagnostic. A `case/in` with several branches, each
# with nested sub-patterns, gives no other clue about WHICH branch got closest
# or which sub-test rejected the value -- ruby's message points straight at it,
# and that is why the message was given this shape (Bug #17925) rather than
# staying generic.
#
# `NoMatchingPatternKeyError` carries the same information structurally, in
# `#key` and `#matchee`, which zeo does answer -- so the data is present and
# only the `NoMatchingPatternError` message is not built from it.

begin
  case 1
  in String then :never
  end
rescue NoMatchingPatternError => e
  puts "#{e.class}: #{e.message}"
end

begin
  case { a: 1 }
  in { a: String } then :never
  end
rescue NoMatchingPatternError => e
  puts "#{e.class}: #{e.message}"
end

begin
  case [1, 2]
  in [Integer, String] then :never
  end
rescue NoMatchingPatternError => e
  puts "#{e.class}: #{e.message}"
end

# The key error already carries its detail.
begin
  { a: 1 } => { b: }
rescue NoMatchingPatternKeyError => e
  puts "#{e.class}: key=#{e.key.inspect} matchee=#{e.matchee.inspect}"
end
