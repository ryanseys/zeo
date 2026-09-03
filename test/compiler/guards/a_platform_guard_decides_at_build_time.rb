# Four more guard shapes that decide at compile time, each gating a NEW
# class -- the shape that has no answer under a compile-time MRO unless
# the condition folds:
#   - `RUBY_PLATFORM.include?('java')` (the JRuby gate spelled without a
#     regexp -- log4r)
#   - `Gem.win_platform?` (chef, isomorfeus)
#   - `X.singleton_class.method_defined?(:m)` (rbs's parser probe)
#   - `Qualified::Path.method_defined?(:m)` (only bare names folded before)
if RUBY_PLATFORM.include?("java")
  class JavaOnly; end
end
puts defined?(JavaOnly).inspect

unless Gem.win_platform?
  class NotWindows
    def hi
      "posix"
    end
  end
end
puts NotWindows.new.hi

module Deep
  class Thing
    def present; end
  end
end

if Deep::Thing.singleton_class.method_defined?(:new)
  class SawTheSingleton
    def hi
      "singleton fold"
    end
  end
end
puts SawTheSingleton.new.hi

if Deep::Thing.method_defined?(:present)
  class SawTheQualifiedPath
    def hi
      "qualified fold"
    end
  end
end
puts SawTheQualifiedPath.new.hi
__END__
nil
posix
singleton fold
qualified fold
