# Issue #266 regression: defining `def length` on a class flipped
# Spinel's inference for IntArray params on unrelated class methods to
# poly. Cause was `infer_cls_meth_param_from_body` committing a param
# to `obj_<C>` whenever the only methods called on it (here `.length`)
# matched a user class — even though IntArray (and every other built-in
# container) also has `.length`. The body signal is too weak to commit;
# the gate added in this fix skips the pre-typing when the called set
# is satisfied by built-in container types, leaving call-site
# unification to decide.

class Mat
  attr_accessor :flat, :nrows, :ncols
  def initialize(nrows, ncols)
    @nrows = nrows
    @ncols = ncols
    @flat  = Array.new(nrows * ncols, 0.0)
  end
end

class Net
  def backward(input_ids, logits)
    n_pred = input_ids.length - 1
    if n_pred <= 0
      return 0.0
    end
    self.cross_entropy_grad(logits, input_ids)
  end

  def cross_entropy_grad(logits, token_ids)
    n     = token_ids.length - 1
    total = 0.0
    i = 0
    while i < n
      target = token_ids[i + 1]
      total += logits.flat[target]
      i += 1
    end
    total
  end
end

def make_ids(s)
  parts = s.split(" ")
  ids   = [parts[0].to_i]
  k = 1
  while k < parts.length
    ids.push(parts[k].to_i)
    k += 1
  end
  ids
end

# The class with `def length` is the trigger — pre-fix this caused
# Net#backward and Net#cross_entropy_grad to be inferred with poly
# `input_ids` / `token_ids` params and then mis-compile.
class Bag
  attr_accessor :data
  def initialize(data); @data = data; end
  def length;           0;             end
end

prompt = make_ids("0 1 2")
puts Net.new.backward(prompt, Mat.new(3, 4))
