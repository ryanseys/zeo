r = Random.new(5)
p((r.rand(Float::INFINITY) rescue $!.class))
p((r.rand(-Float::INFINITY) rescue $!.class))
p((r.rand(Float::NAN) rescue $!.class))
p((r.rand(1.0..Float::INFINITY) rescue $!.class))
p((r.rand(0.0).class rescue $!.class))
p((Random.rand(Float::INFINITY) rescue $!.class))
__END__
Errno::EDOM
Errno::EDOM
Errno::EDOM
Errno::EDOM
Float
Errno::EDOM
