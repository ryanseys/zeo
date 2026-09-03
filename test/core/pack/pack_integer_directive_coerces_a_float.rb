# An integer pack directive truncates a Float toward zero (CRuby coerces
# through an exact Integer, so a value past the i64 range wraps modulo
# 2**64 rather than saturating); NaN/Infinity raise FloatDomainError.

p [1.5].pack("C*").bytes
p [-3.75, 200.9].pack("c2").unpack("c2")
p [2.0e19].pack("Q").unpack1("Q")
p [-2.0e19].pack("q").unpack1("q")
p [1.0e300].pack("Q").unpack1("Q")
begin; [Float::NAN].pack("C"); rescue FloatDomainError => e; puts "nan: #{e}"; end
begin; [-Float::INFINITY].pack("q"); rescue FloatDomainError => e; puts "inf: #{e}"; end
__END__
[1]
[-3, -56]
1553255926290448384
-1553255926290448384
0
nan: NaN
inf: -Infinity
