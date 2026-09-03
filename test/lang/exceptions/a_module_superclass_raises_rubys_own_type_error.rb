# Every built-in CLASS is subclassable (`zeo_abi::NOT_PAYLOAD_ROOTS` is a
# denylist of the shapes that are not payload wrappers, not an allowlist
# of the ones that are). What is left is the two superclasses RUBY
# refuses, and zeo compiles each into ruby's own raise rather than a
# compile error naming zeo.

begin
  class WantsModule < Comparable
  end
rescue TypeError => e
  puts "module: #{e.message}"
end
begin
  class WantsClass < Class
  end
rescue TypeError => e
  puts "class: #{e.message}"
end
__END__
module: superclass must be an instance of Class (given an instance of Module)
class: can't make subclass of Class
