# Ruby refuses a binding inside a `|` alternation -- `in [1, x] | [3, 4]` is
# a SyntaxError -- but it carves out every name beginning with `_`. Both
# `in [301 | 302, :post] | [303, _]` and `in [1, _v] | [9, 9]` are Syntax OK,
# and the local is simply nil when the branch that named it is not the one
# that matched.
#
# zeo treated an underscore name as an ordinary binding and rejected the
# clause, claiming to match real Ruby. It rejected `net/imap`,
# `sass-embedded` and every openai-shaped API client.

def redirect?(status, method)
  case [status, method]
  in [301 | 302, :post] | [303, _]
    "redirect"
  else
    "other"
  end
end

p redirect?(301, :post)
p redirect?(302, :post)
p redirect?(303, :get)
p redirect?(200, :get)

def space(options)
  case options
  in { whiteness: _ } | { blackness: _ }
    "HWB"
  in { saturation: _ } | { lightness: _ }
    "HSL"
  else
    "none"
  end
end

p space({ whiteness: 1 })
p space({ blackness: 1 })
p space({ lightness: 2 })
p space({})

# The name still binds, and reads back nil from the branch that never set it.
def named(v)
  case v
  in [1, _payload] | [9, 9]
    _payload
  else
    :no
  end
end

p named([1, 42])
p named([9, 9])
p named([5, 5])
