$:.unshift File.dirname(__FILE__)
$LOAD_PATH.unshift(File.expand_path("../lib", __FILE__))
$LOAD_PATH << File.dirname(__FILE__)
$:.push File.dirname(__FILE__)
puts 1
