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
__END__
TypeError: no implicit conversion from nil to integer
TypeError: no implicit conversion from nil to integer
TypeError: no implicit conversion from nil to integer
TypeError: no implicit conversion from nil to integer
TypeError: no implicit conversion from nil to integer
TypeError: no implicit conversion from nil to integer
TypeError: no implicit conversion of nil into Array
TypeError: no implicit conversion of Integer into Array
TypeError: no implicit conversion of Symbol into Array
TypeError: no implicit conversion of Integer into Hash
TypeError: no implicit conversion of Integer into Hash
TypeError: no implicit conversion of Integer into String
TypeError: no implicit conversion of Float into String
TypeError: no implicit conversion of nil into String
RangeError: float 1e+300 out of range of integer
RangeError: bignum too big to convert into 'long'
RangeError: bignum too big to convert into 'long'
TypeError: wrong argument type Integer (must respond to :each)
TypeError: no implicit conversion of Integer into String
TypeError: no implicit conversion of Integer into String
