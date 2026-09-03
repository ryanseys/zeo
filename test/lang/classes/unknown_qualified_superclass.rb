# A superclass naming a constant the program never defines.
#
# CRuby evaluates the superclass expression when the definition RUNS, raises
# `NameError` there, and never brings the class into being. zeo does the same by
# rewriting the whole definition to the bare constant read -- but only once it
# agrees the name is unknown, and the test for that read the LAST segment only.
#
# So `class Tilt < ::Tilt::Template` counted as known because some unrelated
# `Template` was assigned somewhere in the program, and the definition was
# registered instead of deferred -- failing later as an unknown superclass
# rather than raising where Ruby raises. temple, wicked_pdf and dotenv all
# write that self-shadowing shape against a gem that may not be installed.

# An unrelated constant that happens to share the leaf.
module Rendering
  Template = "not the one temple means"
end

p Rendering::Template

begin
  class Tilt < ::Tilt::Template
  end
rescue NameError => e
  p e.class
  p e.message.include?("Tilt")
end

# The class never came into being.
p defined?(Tilt).nil?

# A bare leaf is still resolved leniently: `Template` here could name the
# constant above through the lexical chain, so it is NOT deferred -- it is a
# genuine compile-time question, and answering it "unknown" would be wrong.
module Rendering
  class Sub
    def call
      "still compiles"
    end
  end
end

p Rendering::Sub.new.call
__END__
"not the one temple means"
NameError
true
true
"still compiles"
