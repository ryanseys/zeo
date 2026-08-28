# A folded `is_a?` arm in EXPRESSION position must still run its statements in
# order. The fold emitted the arm's leading statements inside a statement
# expression wrapped around its value -- but the value itself, when it is a
# conditional, hoists its branches into the enclosing prelude, which is
# written out first. So the assignment landed BELOW the `if` that reads it,
# the nil arm always won, and a branch reached its tail without having run its
# own first line. (matz/spinel#4139, from a Roundhouse app where four
# controller assertions raised "unknown status :success")
RANGES = { success: 200..299, redirect: 300..399 }.freeze
CODES  = { ok: 200, created: 201 }.freeze

def check(expected, actual)
  if expected.is_a?(Symbol)
    range = RANGES[expected]
    if range.nil?
      code = CODES[expected]
      raise "unknown #{expected.inspect}" if code.nil?
      code == actual
    else
      range.include?(actual)
    end
  else
    expected == actual
  end
end

# The key that IS in the first hash: the fold's arm has to read the value its
# own first statement assigned.
p check(:success, 250)
p check(:success, 404)
p check(:redirect, 302)
# A key that is only in the second hash still reaches the nil arm correctly.
p check(:ok, 200)
p check(:created, 200)
# The non-Symbol path is untouched by the fold.
p check(200, 200)
p check(200, 404)

# Two leading statements, so the ordering is pinned rather than coincidental.
def two_leading(v)
  if v.is_a?(Integer)
    a = v * 2
    b = a + 1
    if b > 10
      "big:#{b}"
    else
      "small:#{b}"
    end
  else
    "other"
  end
end
p two_leading(1)
p two_leading(9)
p two_leading("x")

# A leading statement in front of a plain expression, not a conditional.
def leading_then_value(v)
  if v.is_a?(String)
    n = v.length
    n * 3
  else
    -1
  end
end
p leading_then_value("abcd")
p leading_then_value(7)
