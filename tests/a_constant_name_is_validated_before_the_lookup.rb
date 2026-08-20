# `rb_is_const_id` runs BEFORE the lookup: a string that is not a constant
# name raises `wrong constant name`, carrying the name, rather than reporting
# an `uninitialized constant` miss. zeo checked only the FIRST character, so
# `Bad.Name` and `Bad Name` passed the gate and came back as misses -- and
# `const_defined?` ran no check at all.
#
# A `::`-separated PATH is still a legal argument, each segment checked on its
# own, and `**nil` below is the other "no-op spelled as a value" this pass
# fixed: it means "pass no keywords", never a conversion.

module Holder
  Inner = Module.new
  Deep = 1
end

[
  "Bad.Name", "Bad Name", "lower", "_Under", "9Nine", "", "Holder::lower",
  "Holder::Nope", "Holder::Deep", "Holder", "Holder::Inner",
].each do |name|
  begin
    p [name, Object.const_get(name)]
  rescue NameError => e
    p [name, e.class, e.message]
  end
end

# `const_defined?` and `const_source_location` run the same gate.
["Bad.Name", "lower", "Holder"].each do |name|
  begin
    p Object.const_defined?(name)
  rescue NameError => e
    p [:defined, e.message]
  end
  begin
    p Object.const_source_location(name).nil?
  rescue NameError => e
    p [:location, e.message]
  end
end

# A Symbol argument goes through the same check, and its `#name` is the
# symbol it was given.
begin
  Object.const_get(:"Bad Name")
rescue NameError => e
  p [e.message, e.name]
end

# --- `**nil` -----------------------------------------------------------------
def kw(a, b: 2, **rest) = [a, b, rest]
p kw(1, **nil)
p kw(2, **nil, b: 3)
h = nil
p kw(4, **h)
def anykw(**k) = k
p anykw(**nil)
