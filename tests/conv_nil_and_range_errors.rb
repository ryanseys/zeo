# The exact CRuby error shapes: the special lowercase nil message at
# NUM2LONG sites, the generic shapes elsewhere, RangeError for
# out-of-range floats and bignums.
def err
  yield
rescue => e
  puts "#{e.class}: #{e.message}"
end

err { [1, 2] * nil }
err { [1, 2, 3][nil] }
err { [1, 2, 3].first(nil) }
err { [1, 2, 3].take(nil) }
err { "ab" * nil }
err { "abc".split(",", nil) }
err { [1] + nil }
err { [1] + 5 }
err { [1].concat(:x) }
err { {} < 1 }
err { {}.merge(1) }
err { "a" + 1 }
err { "a" << 1.5 }
err { "a" << nil }
err { [1, 2, 3][1e300] }
err { [1, 2, 3].first(2**64) }
err { "x" * (2**64) }
err { [1, 2].zip(1) }
err { [1].pack(1) }
err { "A".unpack(1) }
