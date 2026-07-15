# The Ruby-visible surface of the base64 native package (Phase 14.3): each
# native_func is implemented as spinelc_base64::<name> in the workspace
# crate named by spin.toml's [native] table, linked only into programs
# that actually `require "base64"`. The type constants are documentation
# (arity is what the compiler checks) -- see HirNode::NativeFunc's docs.
module Base64
  native_crate "spinelc_base64"
  native_func :encode64, [String], String
  native_func :decode64, [String], String
  native_func :strict_encode64, [String], String
  native_func :strict_decode64, [String], String
  native_func :urlsafe_encode64, [String], String
  native_func :urlsafe_decode64, [String], String
end
