# `respond_to_missing?` is wired into the whole `respond_to?` surface
# (the codegen folds and the Kernel row), `method(:dyn)` succeeds when the
# hook admits the name (its Method dispatches through `method_missing`),
# and `defined?(recv.dyn)` reports "method". The default `method_missing`/
# `respond_to_missing?` rows are hidden-private (invisible to `methods`,
# answer `respond_to?` only with `include_all`) and are what an override's
# `super` reaches -- a bare `super` in `method_missing` raises the real
# NoMethodError. Output oracle-verified verbatim.

class R
  def respond_to_missing?(name, include_private = false)
    name.to_s.start_with?("dyn_") || super
  end
  def method_missing(name, *args)
    if name.to_s.start_with?("dyn_")
      "handled #{name}"
    else
      super
    end
  end
end
r = R.new
p r.respond_to?(:dyn_foo)
p r.respond_to?(:other)
p r.dyn_foo
p defined?(r.dyn_foo)
p defined?(r.other)
m = r.method(:dyn_bar)
p m.call
begin
  r.nope_at_all
rescue NoMethodError => e
  puts e.message
end
o = Object.new
p o.respond_to?(:method_missing)
p o.respond_to?(:method_missing, true)
p Object.new.methods.include?(:respond_to_missing?)
puts "done"
__END__
true
false
"handled dyn_foo"
"method"
nil
"handled dyn_bar"
undefined method 'nope_at_all' for an instance of R
false
true
false
done
