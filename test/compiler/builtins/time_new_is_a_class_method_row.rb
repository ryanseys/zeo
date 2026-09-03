p Time.method(:new).owner
p Time.singleton_methods(false).sort
p [Regexp.method(:new).owner, Hash.method(:new).owner, Range.method(:new).owner]
p [Array.method(:new).owner, String.method(:new).owner]
__END__
Class
[:at, :gm, :local, :mktime, :now, :utc]
[Class, Class, Class]
[#<Class:Array>, #<Class:String>]
