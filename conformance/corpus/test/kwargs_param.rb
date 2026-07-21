# KeywordRestParameterNode -- def f(**kw)
#
# Declares a method that captures all keyword arguments into a single
# sym_poly_hash slot. The full body-access path (kw[:key], kw.length)
# is deferred until a real program needs it; this test verifies the declaration
# alone compiles and the existing **nil form still works.
#
# Forwarding usage (**kw → another method's **kw slot) is exercised by
# test/forwarding_args.rb, which routes through cluster A3 + B1.

# Declaration compiles; body never reads kw, so no kwargs runtime path
# is needed. Validates that adding the param slot doesn't break
# bootstrap or alter the C signature for callers that don't pass kwargs.
def silent(a, **kw)
  a + 1
end
puts silent(2).to_s   #=> 3

# `def f(**nil)` is the explicit "rejects keyword args" form.
# Verifies the existing NoKeywordsParameterNode handling still works
# after the parser's PARAMETERS_NODE flattener gained the keyword_rest
# emission (cluster A1).
def strict(a, b, **nil)
  a + b
end
puts strict(2, 3).to_s   #=> 5
