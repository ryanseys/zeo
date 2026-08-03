# A definition hook sees a half-built class through REFLECTION -- zeo's
# watermark handles that, and `tests/method_added_watermark.rb` covers it. What
# it does not handle is DISPATCH: calling a method the hook has not been told
# about yet succeeds here, where ruby raises NoMethodError.
#
# The two are different machines. Reflection asks `runtime_meta::not_yet_defined`
# on the way to an answer it was already computing; dispatch resolves through
# the inline cache and the flattened method tables, where the same question
# would be a thread-local check on the hot path of every call in the program.
#
# Fix shape, if this ever earns it: nothing cheap. The honest version defers
# method-table registration to the class body's own position instead of doing it
# all in `main()`, which is a far larger change than the hooks themselves and
# would slow every program's startup to serve a case no real code exercises.

class X
  def self.method_added(name)
    return unless name == :one
    begin
      puts "  two -> #{new.two}"
    rescue NoMethodError => e
      puts "  #{e.class}"
    end
  end

  def one = 1
  def two = 2
end

# The same asymmetry through `send`.
class Y
  def self.method_added(name)
    return unless name == :a
    begin
      puts "  b -> #{new.send(:b)}"
    rescue NoMethodError => e
      puts "  #{e.class}"
    end
  end

  def a = 1
  def b = 2
end
