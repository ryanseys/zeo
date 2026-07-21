# Sibling to #615 (poly_local_is_a_string_narrow_in_non_tail_if):
# the is_a? narrow + LV-read unbox must also fire inside the y1
# inlined specialization that block-passing creates. The
# non-inlined `sp_Base_check` body already unboxed correctly after
# #615 / #817149f; the inlined y1 copy emitted the raw poly local
# where `const char *` is expected. Issue #624.

class Base
  def check(content = nil, &block)
    if content.is_a?(String)
      pattern = Regexp.new(Regexp.escape(content))
      raise "no match" unless pattern.match?(content)
    end
    yield if block
  end
end

class Sub < Base
  def run
    check("hello")
    check(123)        # makes content's static type poly
    check { nil }     # block-passing call triggers y1 inlining
  end
end

Sub.new.run
puts "OK"
