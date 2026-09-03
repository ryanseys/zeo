p [1, 2].to_enum(:each).size
p 3.times.to_enum.size
p [1, 2].enum_for(:each) { 99 }.size
__END__
nil
nil
99
