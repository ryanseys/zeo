# Two NaN floats do not compare identical under equal?/eql? the way ruby's
# boxed NaN does.
#
n001 = Float::NAN
h001 = { n001 => 1 }
p h001[n001]
p n001.equal?(n001)
p [n001].include?(n001)
p n001.eql?(n001)
p(n001.hash == n001.hash)
p(n001 == n001)
__END__
1
true
true
false
true
false
