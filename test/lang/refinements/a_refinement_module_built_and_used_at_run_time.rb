# zeo rewrites refinements lexically over a named module's refine blocks, so a
# Module.new has no refine on it and `using` has nothing to apply.
class Probe
  using Module.new {
    refine ::String do
      def shout = upcase + "!"
    end
  }

  def self.run = "hi".shout
end

p Probe.run
__END__
"HI!"
