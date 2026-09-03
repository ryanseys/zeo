require "strscan"
sc = StringScanner.new("foo123bar")
puts sc.scan(/[a-z]+/)
puts sc.scan(/\d+/)
puts sc.rest
sc2 = StringScanner.new("a=1;b=2")
puts sc2.scan_until(/;/)
__END__
foo
123
bar
a=1;
