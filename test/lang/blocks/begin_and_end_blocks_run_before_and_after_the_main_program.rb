# BEGIN runs first in SOURCE order; END runs at exit in REVERSE order,
# which is `at_exit` exactly -- so END lowers to one.

puts "main 1"
END { puts "end A" }
BEGIN { puts "begin A" }
puts "main 2"
END { puts "end B" }
BEGIN { puts "begin B" }
puts "main 3"
__END__
begin A
begin B
main 1
main 2
main 3
end B
end A
