# The shape tests/gaps/a_runtime_unit_may_yield_from_a_singleton_body.rb is
# about: a `yield` in a `def` in a `class << self` body. `gems/time/lib/time.rb`
# writes `Time.strptime` exactly this way.
module SingletonYield
  class << self
    def take(x)
      x = yield(x) if block_given?
      x
    end
  end
end
