# Complex#real and #imaginary through map, including an array mixing Complex with an Integer.
p [Complex(1, 2), Complex(3, 4)].map { |z| z.real }
p [Complex(1, 2), Complex(3.5, 4)].map { |z| z.imaginary }
p [Complex(1.0, 2), 5].map { |z| z.real }
__END__
[1, 3]
[2, 4]
[1.0, 5]
