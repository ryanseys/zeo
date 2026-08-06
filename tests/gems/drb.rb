# drb vendored (with observer, its dependency). Loading it needs
# `undef :to_a if respond_to?(:to_a)` under a runtime guard and
# `Thread.new(&proc)`; no service is started here.
require "drb"

p DRb::VERSION.is_a?(String)
p DRb.respond_to?(:start_service)
p DRb.respond_to?(:current_server)
p DRbUndumped.instance_of?(Module)
p DRb::DRbIdConv.new.class
p DRb::DRbServer.instance_methods(false).sort.first(3)
p DRb::DRbObject.respond_to?(:new_with_uri)

# The error hierarchy is plain Ruby and fully usable offline.
p DRb::DRbConnError.ancestors.include?(DRb::DRbError)
p DRb::DRbBadURI.ancestors.include?(DRb::DRbError)
begin
  DRb::DRbProtocol.uri_option("bogus:///x", {})
rescue DRb::DRbBadURI, DRb::DRbBadScheme => e
  puts e.class
end

# `DRbObject` over a plain object round-trips its id conversion.
conv = DRb::DRbIdConv.new
obj = Object.new
p conv.to_obj(conv.to_id(obj)).equal?(obj)
