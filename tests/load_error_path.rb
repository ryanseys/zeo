# `LoadError#path` names the feature that would not load. It is one of the
# typed introspection slots an exception carries beside its message, and the
# raise site fills it; a hand-built LoadError leaves it nil rather than raising
# the way an unset `KeyError#key` does.
begin
  require "definitely_not_a_gem"
rescue LoadError => e
  p e.path
  p e.message
end

p LoadError.new("hand built").path
p LoadError.new.path
p LoadError.instance_method(:path).owner
p LoadError.new("x").respond_to?(:path)

# A subclass inherits the reader, and a raise through it fills the same slot.
class VendorMissing < LoadError; end
p VendorMissing.new("nope").path
begin
  raise VendorMissing, "no vendor"
rescue LoadError => e
  p [e.class, e.path]
end

# `load` of a missing file records its argument too.
begin
  load "no/such/script.rb"
rescue LoadError => e
  p e.path
end

# The slot is hidden, like the message slot -- it is not an ivar.
begin
  require "another_missing_one"
rescue LoadError => e
  p e.instance_variables
end

# A sibling ScriptError has no `#path`.
p NotImplementedError.new("x").respond_to?(:path)
p ArgumentError.new("x").respond_to?(:path)
