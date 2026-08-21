# Two questions a backtrace answers that zeo used to answer differently
# from ruby, both about WHERE a frame is rather than what it does.
#
# (1) The scope a block hangs off. A `require` zeo splices into its
#     requiring file still has a scope of its own: ruby names a required
#     file's top level `<top (required)>`, never the requiring file's
#     `<main>`. It is not cosmetic -- a library filters its own frames out
#     of a backtrace by path and by name.
#
# (2) The line a CALL is reported at. Ruby reports the line the method
#     NAME sits on, which for a chained or block-receiver call is not the
#     line the expression starts on. Coverage still counts the first line;
#     the two are different questions and are answered separately.
require_relative "lib/backtrace_scopes"

def show(label)
  yield
rescue => e
  puts "#{label}:"
  puts e.backtrace.map { |f| "  #{f.sub(__dir__ + "/", "")}" }
end

show("blocks at a required file's top level") { NESTED_BLOCKS.call }
show("a method in a required file") { a_method_here }

show("a chained call") do
  Chained
    .boom
end

show("a call on a block's value") do
  [1].map do |i|
    i
  end.frozen_method_that_is_missing
end

# The receiver and the arguments keep their OWN lines: a raise while they
# are being evaluated is reported where it was written, not at the name.
def raises_while_evaluating = raise("raised building the receiver")
show("a raise in the receiver of a chained call") do
  raises_while_evaluating
    .to_s
end
