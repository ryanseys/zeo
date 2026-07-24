# Sibling case to poly_local_is_a_string_narrow_in_tail_if (#612):
# a poly-typed local narrowed via `if content.is_a?(String) ... end`
# when the if is NOT in tail position. Before this fix
# compile_if_stmt didn't push the is_a? narrow, so a typed-arg
# callee inside the body (sp_re_escape's `const char *`) saw the
# raw sp_RbVal and tripped a C compile. The fix mirrors
# compile_cond_return's narrow push and also switches
# compile_stmt's LV-write to use the slot's declared type so a
# rewrite of the narrowed var inside the body picks the slot arm,
# not the narrow's pseudo-type. Issue #615.

class Resp
  attr_reader :body
  def initialize(b); @body = b; end
end

class Tester
  attr_accessor :response

  def assert_select(selector, content_or_opts = nil, &block)
    body = response.body
    if content_or_opts.is_a?(Hash)
      content = nil
    else
      content = content_or_opts
    end
    if content.is_a?(String)
      pattern = Regexp.new("<#{selector}>#{Regexp.escape(content)}<")
      raise "no match" unless pattern.match?(body)
    end
    yield if block
  end
end

t = Tester.new
t.response = Resp.new("<h1>Title<")
t.assert_select("h1", "Title")
t.assert_select("div", { count: 1 })
puts "OK"
