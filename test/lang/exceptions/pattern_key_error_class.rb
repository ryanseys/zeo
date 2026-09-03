# A hash pattern that misses a KEY raises `NoMatchingPatternKeyError` in CRuby,
# carrying the key it wanted and the Hash it was asked of. zeo raised the parent
# `NoMatchingPatternError` with no detail.
#
# A pattern compiles to ONE boolean, so by the time the `expr => pattern` arm
# raises, the expression tree that knew which key was absent is gone. The miss
# is recorded where it happens and read back at the raise
# (`zeo_rt::pattern_match_error`), and the record is cleared at the start of
# every required match so a `case/in` arm that simply did not apply cannot leak
# into a later failure.
#
# A key that is PRESENT with a non-matching value is a different failure and
# keeps the parent class, which is why the two are kept apart at the check.

def t(label)
  yield
  puts "#{label}: no raise"
rescue NoMatchingPatternError => e
  key = e.respond_to?(:key) ? e.key.inspect : "-"
  matchee = e.respond_to?(:matchee) ? e.matchee.inspect : "-"
  puts "#{label}: #{e.class} key=#{key} matchee=#{matchee}"
end

t("missing key") { { a: 1 } => { b: } }
t("missing, several") { { a: 1 } => { a:, b:, c: } }

# A NESTED miss names the inner key and the inner hash -- but the message still
# quotes the outermost subject.
t("nested missing") { { a: { b: 1 } } => { a: { c: } } }

# Everything that is not a missing key stays the parent class.
t("wrong value") { { a: 1 } => { a: 2 } }
t("array length") { [1] => [1, 2] }
t("not a hash") { 5 => { a: } }

# A match that SUCCEEDS leaves nothing behind for the next one to read.
t("ok") { { a: 1 } => { a: } }
t("after ok") { { a: 1 } => { z: } }

# ...and neither does a `case/in` arm that merely did not apply: the array
# failure below must not report itself as a key error.
t("case then array fail") do
  case { a: 1 }
  in { q: } then :q
  in { a: } then :a
  end
  [1] => [1, 2]
end

# `case/in` itself never raises the key error -- a missing key just means that
# arm does not match.
result = case { a: 1 }
         in { b: } then :b
         in { a: } then :a
         end
p result

begin
  case { a: 1 }
  in { b: } then :b
  end
rescue NoMatchingPatternError => e
  p e.class
end

p NoMatchingPatternKeyError.ancestors.include?(NoMatchingPatternError)
p NoMatchingPatternKeyError.new(matchee: { a: 1 }, key: :b).key
__END__
missing key: NoMatchingPatternKeyError key=:b matchee={a: 1}
missing, several: NoMatchingPatternKeyError key=:b matchee={a: 1}
nested missing: NoMatchingPatternKeyError key=:c matchee={b: 1}
wrong value: NoMatchingPatternError key=- matchee=-
array length: NoMatchingPatternError key=- matchee=-
not a hash: NoMatchingPatternError key=- matchee=-
ok: no raise
after ok: NoMatchingPatternKeyError key=:z matchee={a: 1}
case then array fail: NoMatchingPatternError key=- matchee=-
:a
NoMatchingPatternKeyError
true
:b
