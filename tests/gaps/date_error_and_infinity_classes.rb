# `require "date"` defines two classes zeo does not: `Date::Error` (an
# `ArgumentError` subclass every invalid-date raise uses) and `Date::Infinity`.
#
# Both need a FEATURE-GATED exception/class id. zeo's exception hierarchy is
# `zeo_abi::EXCEPTION_CLASSES`, which is unconditional -- a row added there
# would make `Date::Error` resolve BEFORE `require "date"`, which is its own
# divergence. And `zeo_abi::BUILTINS` rows do carry `feature: Some("date")`,
# but nothing gives such a row an exception CONSTRUCTOR, so
# `dispatch::construct_exception("Date::Error", ..)` would not find one and
# `raise Date::Error` could not build an instance.
#
# The fix is a feature-gated exception row: a `BUILTINS` entry whose
# superclass is an `EXCEPTION_CLASSES` id, plus the constructor edge in
# `ClassRegistry::register` that today only the exception block installs.
# Until then every invalid-date raise is a plain `ArgumentError` with CRuby's
# exact message, so `rescue ArgumentError` and `rescue => e` both catch it and
# only `rescue Date::Error` does not.
require "date"

p Date::Error.ancestors.include?(ArgumentError)
begin
  Date.new(2024, 2, 30)
rescue => e
  p e.class
end
p defined?(Date::Infinity)
