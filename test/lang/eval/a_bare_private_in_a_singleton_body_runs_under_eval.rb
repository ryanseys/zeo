# A `class << self` body holding a BARE `private` compiles inside a run-time
# `eval`.
#
# A class body written in a snippet runs as one more `class_eval` of its own
# SOURCE TEXT, sliced from its statements' spans, so a statement with no span
# cannot be re-emitted at all. A bare visibility directive in a singleton body
# survives lowering as a marker plus a synthesized `public` reset at body end,
# and that reset used to be stamped `Span::SYNTH` -- which refused the whole
# compile. rubygems' `platform.rb` is the corpus case: its `class << self`
# holds a bare `private` two thirds of the way down.

src = <<~RUBY
  class Widget
    class << self
      def build
        "built"
      end

      private

      def helper
        "helper"
      end
    end
  end
RUBY

eval src

p Widget.build
p Widget.singleton_class.private_method_defined?(:helper)
begin
  Widget.helper
rescue NoMethodError => e
  p e.class
end
# The cursor dies with the body, exactly as CRuby's dies with the cref: a
# `def self.x` written after it is public again.
class Widget
  def self.after
    "after"
  end
end
p Widget.after
__END__
"built"
true
NoMethodError
"after"
