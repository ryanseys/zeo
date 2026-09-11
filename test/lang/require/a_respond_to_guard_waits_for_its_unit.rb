# A `respond_to?` guard on a class method that only a unit which has not
# run yet defines.
module R; end
p R.respond_to?(:configure)
%w[core].each { |f| require_relative "a_respond_to_guard_waits_for_its_unit/#{f}" } if ARGV.include?("load")
p R.respond_to?(:configure)
R.configure { |c| p c } if R.respond_to?(:configure)
p :done
__END__
false
false
:done
