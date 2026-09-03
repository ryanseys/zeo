# A class-body `if`/`unless` whose guard is compile-time decidable (a
# target-version or feature-probe gate) and whose taken branch holds a whole
# definition (`class`/`module`/`include`/`prepend`) is inlined into that branch,
# exactly as CRuby's load-time reachability decides it. The taken branch's
# nested definition registers like an ordinary class-body one; a dropped branch
# leaves nothing behind. (This is the class-body analogue of the top-level
# conditional-definition handling.)
class Host
  # "4.0.6" >= "3.0" is true -> Modern registers.
  if RUBY_VERSION >= "3.0"
    class Modern
      def tag
        "modern"
      end
    end
  end

  # "4.0.6" < "3.0" is false -> the legacy nested class is dropped entirely.
  if RUBY_VERSION < "3.0"
    class Legacy
      def tag
        "legacy"
      end
    end
  end

  # An `unless` over a true guard drops its branch too.
  unless RUBY_VERSION >= "3.0"
    module NeverMixed
      def extra; end
    end
    include NeverMixed
  end
end

puts Host::Modern.new.tag
puts defined?(Host::Legacy).inspect
puts Host.ancestors.include?(Host::NeverMixed) rescue puts "NeverMixed undefined"
__END__
modern
nil
NeverMixed undefined
