# The exception stack's mark window follows sp_exc_top up and down, and a slot
# that leaves it kept its pointer. Pop below a used slot, collect (the object
# is now unreferenced, so it is freed), then push back over it, and the window
# covers a dangling pointer again -- the collector then walks a header that the
# allocator has since handed to a string.
#
# The slots above the window are cleared where the window is defined, so after
# any collection a slot outside it holds nothing. This exercises the shape:
# nesting that oscillates across a used depth, with allocation in between.

class MissingTemplate < StandardError; end

def churn(i)
  s = +""
  10.times { |j| s << "<input name=\"f#{i}_#{j}\" value=\"_csrf_token\">" }
  s.length
end

def deep(n, i)
  return (raise MissingTemplate, "t#{i}") if n == 0
  begin
    deep(n - 1, i)
  rescue MissingTemplate => e
    churn(i)
    raise e if n > 1
    e.message.length
  end
end

total = 0
2000.times do |i|
  d = (i % 3) + 1
  begin
    total += deep(d, i)
  rescue MissingTemplate
    total += 1
  end
  total += churn(i)
end
p total > 0
