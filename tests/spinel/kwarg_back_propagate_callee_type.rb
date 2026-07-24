# #561 (Sam Ruby). Kwarg passthrough back-propagation: when
# method outer forwards `inner(name: lvar)` and inner's
# `name:` is typed (default value or body usage), outer's
# matching kwarg should pick up the callee's slot type
# rather than defaulting to int. Pre-fix outer.s stayed int
# and the C compile failed at the int -> const char *
# coercion at the inner call site.
#
# This mirrors the positional callee_slot back-propagation
# pass but keyed on kwarg name rather than position; the
# kwarg path also accepts any concrete callee type
# (string / int_array / hash variant / etc.) rather than
# just ptr / obj.

module Broadcasts
  def self.append(stream:, target:, html:)
    record(action: :append, stream: stream, target: target, html: html)
  end

  def self.record(action:, stream:, target:, html:)
    out = action.to_s + "|" + stream + "|" + target + "|len=" + html.length.to_s
    out
  end
end

puts Broadcasts.append(stream: "s", target: "t", html: "<div>")
