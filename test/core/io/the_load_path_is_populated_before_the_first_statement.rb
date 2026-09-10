# `$:.unshift` and `$LOAD_PATH.unshift` on line 1 reach a real array, not nil.
$:.unshift File.dirname(__FILE__)
$LOAD_PATH.unshift(File.expand_path("../lib", __FILE__))
$LOAD_PATH << File.dirname(__FILE__)
$:.push File.dirname(__FILE__)
puts 1
__END__
1
