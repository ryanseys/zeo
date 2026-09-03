# `EE.new` for a user `class EE < Exception` constructs a native-backed
# `RubyValue` (no generated struct exists) -- but registration-time type
# inference consulted the not-yet-linearized `ancestors` and typed the
# local as an unboxed struct handle, baking `new_handle` calls to a type
# codegen never emits (invalid Rust, from the timeout gem's
# `ExitException`). The ancestry predicates now walk the recorded
# superclass links, which exist from registration.

module T
  class EE < Exception
  end
  class Er < RuntimeError
    def self.go(m)
      exc = EE.new(m)
      yield exc
    end
  end
end
T::Er.go("x") { |e| puts e.class; puts e.message }
__END__
T::EE
x
