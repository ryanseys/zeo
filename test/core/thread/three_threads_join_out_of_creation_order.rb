t1 = Thread.new { 1 }
t2 = Thread.new { 2 }
t3 = Thread.new { 3 }
puts t3.value + t1.value + t2.value
__END__
6
