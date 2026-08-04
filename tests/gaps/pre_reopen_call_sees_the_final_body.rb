# A call that executes BEFORE a class is reopened must run the body that
# exists at that moment. zeo compiles one Rust fn per surviving method row
# (last-def-wins across the whole program), so the first call below already
# dispatches to the reopened body. Fixing this for real means versioned
# method emission with position-aware call sites, or routing reopened
# redefinitions through the runtime overlay at their execution point -- the
# same timeline family as method_added_sees_the_superseded_body.rb (and, in
# the same shape, visibility_change_in_a_reopened_class.rb).
class Early
  def word
    "first"
  end
end

puts Early.new.word

class Early
  def word
    "second"
  end
end

puts Early.new.word
