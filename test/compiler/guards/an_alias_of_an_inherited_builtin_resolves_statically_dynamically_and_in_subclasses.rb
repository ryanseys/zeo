# The general (non-raise) family: `dup!` for Object's `dup` becomes a
# registry NAME-INDIRECTION row (`register_alias`), consulted on the send
# MISS paths -- so a static call, a dynamic `send`, a subclass receiver,
# and an alias OF an alias all resolve. `respond_to?` answers through the
# source name, inheriting its visibility (`dup!` public, `raise!` private
# like `raise` itself). All outputs oracle-verified.

class Dupper
  alias_method :dup!, :dup
  alias_method :dup2!, :dup!
  alias_method :raise!, :raise
end
class SubDupper < Dupper; end
puts Dupper.new.dup!.class
puts SubDupper.new.dup2!.class
puts SubDupper.new.send(:dup!).class
puts Dupper.new.respond_to?(:dup!)
puts Dupper.new.respond_to?(:raise!)
__END__
Dupper
SubDupper
SubDupper
true
false
