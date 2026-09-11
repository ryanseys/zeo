# A file that reopens a class another compiled-in file defines: its methods
# arrive when that file runs, not before.
require_relative "a_reopen_in_another_unit_waits_for_its_own_file/base"

k = K.new
p k.base
p k.respond_to?(:extra)
p K.instance_methods(false).sort
begin
  k.extra
rescue NoMethodError => e
  puts e.message
end
p K.respond_to?(:extra_class)
begin
  K.extra_class
rescue NoMethodError => e
  puts e.message
end
__END__
:base
false
[:base]
undefined method 'extra' for an instance of K
false
undefined method 'extra_class' for class K
