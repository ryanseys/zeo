# A runtime-name send in an unreachable method must NOT abort compilation.
# Dead code is pruned before codegen, so the send is never emitted; only a
# send that codegen actually emits is diagnosed. This mirrors a real program
# that defines a dispatch helper using `send` but never calls it on the
# compiled entry path.
class Pad
  def press(button)
    "pressed #{button}"
  end

  # Never called -> dead-code-eliminated. Uses a runtime method name (the
  # `action` parameter), which would otherwise trigger the diagnostic.
  def dispatch(action, button)
    send(action, button)
  end
end

puts Pad.new.press(:a)
