# A numeric conversion in format/sprintf answered 0 for an argument that is not
# a number at all -- nil, a Symbol, an Array -- where CRuby refuses (#4010).
# %s accepts anything and is unaffected.
[nil, true, false, :s, [1], { a: 1 }].each do |v|
  ["%d", "%f", "%x"].each do |spec|
    begin
      p format(spec, v)
    rescue TypeError => e
      puts "#{spec} #{v.inspect} => #{e.class}: #{e.message}"
    end
  end
  p format("%s", v)
end

# found while fixing it: a BIGNUM does not fit the long long the conversions
# format, and truncating printed 0 for every value past 64 bits
p format("%d", 2**70)
p format("%x", 2**70)
p format("%o", 2**70)
p format("%b", 2**70)
p format("%d", -(2**70))
p format("%f", 2**70)
p format("%+d", 2**70)
p format("%#x", 2**70)
p format("%30d|", 2**70)
p format("%-30d|", 2**70)

# and a String argument reached the float conversions as a zero
p format("%f", "7")
p format("%e", "7")

# the ordinary conversions keep working
p format("%d", 5)
p format("%d", 2.5)
p format("%d", "7")
p format("%05.2f", 2.5)
p format("%x", 255)
p sprintf("%s-%d", "a", 1)
