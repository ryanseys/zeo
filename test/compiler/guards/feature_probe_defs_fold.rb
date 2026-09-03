# Feature-probe guards fold against the compiled method tables, exactly as CRuby
# decides them at load time. rubygems/bundler gate compat reopens on whether a
# class already has a method -- e.g. bundler's
#   VALIDATES_FOR_RESOLUTION = Specification.new.respond_to?(:validate_for_resolution).freeze
#   unless VALIDATES_FOR_RESOLUTION
#     class SpecificationPolicy ... end
#   end
# On a target whose Specification HAS the method, the boolean constant is true,
# so the compat reopen is dropped (which zeo cannot express at runtime under
# static MRO). This fixture mirrors that shape in a collision-free namespace.
module App
  class Spec
    def validate!
      "real"
    end
  end

  # respond_to? on an instance folds through the method table; `.freeze` is a
  # no-op on the boolean, so the constant is a compile-time `true`.
  HAS_VALIDATE = Spec.new.respond_to?(:validate!).freeze

  class Policy
    def kind
      "modern"
    end
  end

  # HAS_VALIDATE is true -> `unless true` -> the legacy reopen folds away, so
  # `kind` stays the modern body (the observable proof the guard folded).
  unless HAS_VALIDATE
    class Policy
      def kind
        "legacy"
      end
    end
  end
end

puts App::Policy.new.kind
puts App::HAS_VALIDATE
__END__
modern
true
