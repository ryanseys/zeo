r001 = (1.step(10, 0) { |i001| break } rescue $!.class); p r001
r002 = (1.0.step(2.0, 0.0) { |i002| break } rescue $!.class); p r002
v003 = 1; w003 = 0; r003 = (v003.step(10, w003) { |i003| break } rescue $!.class); p r003
r004 = (1.step(by: 0, to: 10) { |i004| break } rescue $!.class); p r004
r005 = (1.step(nil, 0) { |i| break } rescue $!.class); p r005
p(1.step(10, 3).to_a)
p(10.step(1, -3).to_a)
p(1.step(2, 0.25).to_a)
p(1.step(by: 0.5, to: 3).to_a)
p((1.step(10, 0).to_a.size rescue $!.class))
p((1.0.step(2.0, 0.0).to_a.size rescue $!.class))
__END__
ArgumentError
ArgumentError
ArgumentError
ArgumentError
ArgumentError
[1, 4, 7, 10]
[10, 7, 4, 1]
[1.0, 1.25, 1.5, 1.75, 2.0]
[1.0, 1.5, 2.0, 2.5, 3.0]
ArgumentError
ArgumentError
