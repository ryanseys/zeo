# An anonymous refinement module built and used at RUNTIME. Zeo implements
# refinements as a compile-time lexical rewrite over a NAMED module's `refine`
# blocks, so `Module.new` produces a plain runtime module with no `refine` on
# it, and `using` has no compile-time module to apply.
#
# This is the one thing still standing between zeo and irb. The gem is
# vendored and compiles CLEAN -- zero rustc errors, down from 51 -- and runs
# deep into its own initialization before stopping at completion.rb:164,
# which is written exactly like this. Closing it means teaching the existing
# rewrite this shape: an anonymous `Module.new { refine C do ... end }` passed
# straight to `using`, which is a bounded extension rather than a new
# mechanism.
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
