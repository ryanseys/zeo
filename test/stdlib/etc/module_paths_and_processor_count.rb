require "etc"
puts Etc.sysconfdir.class
puts Etc.systmpdir.class
puts Etc.nprocessors.class
puts Etc.nprocessors > 0
puts(Etc.getlogin.is_a?(String) || Etc.getlogin.nil?)
__END__
String
String
Integer
true
true
