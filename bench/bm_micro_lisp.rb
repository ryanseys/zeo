# Micro Lisp: tokens -> ast -> eval over a Scheme-shaped subset
# (numbers, symbols, pairs, lambdas, define, if, recursive calls).
# Polymorphism-rich by construction: every value kind is its own
# class (IntV / SymV / PairV / LambdaV), and the recursive walker
# eval(form, env) returns each of them at different call sites, so
# eval's return widens to poly. Pair head/tail and env values store
# any kind, so those ivars are poly too. Each is_a? dispatch is a
# place where backward-inference (narrowing the return inside a
# branch where the recv is known) could in principle pin a tighter
# type.
#
# Test program is `factorial 6` -> 720. Output is one line so the
# bench harness diffs trivially against CRuby.
#
# attr_accessor (not attr_reader) on every value class is
# deliberate: spinel's value-type detection otherwise turns
# IntV / SymV into in-register structs that can't be stored
# uniformly in a poly array.

class IntV
  attr_accessor :n
  def initialize(n); @n = n; end
end

class SymV
  attr_accessor :s
  def initialize(s); @s = s; end
end

class PairV
  attr_accessor :head
  attr_accessor :tail
  def initialize(h, t); @head = h; @tail = t; end
end

class LambdaV
 # Store the param list as the original PairV form rather than as
 # a precomputed str_array. The str_array approach widened to poly
 # via a chain: collect_param_names' element type infers as poly
 # because the is_a?(SymV) narrow on cur.head doesn't reach the .s
 # at the push site, so @params becomes poly_array, pnames[i] in
 # bind dispatches polyly, and Env.define's `n` widens to poly.
 # Walking the PairV directly keeps the local read of h.s under a
 # tight is_a?(SymV) narrow that does resolve to string.
  attr_accessor :params_form
  attr_accessor :body
  attr_accessor :env
  def initialize(p, b, e); @params_form = p; @body = b; @env = e; end

  def bind(args)
    new_env = Env.new(@env)
    p_cur = @params_form
    a_cur = args
    while p_cur != nil && p_cur.is_a?(PairV) && a_cur != nil && a_cur.is_a?(PairV)
      hh = p_cur.head
      if hh.is_a?(SymV)
        new_env.define(hh.s, a_cur.head)
      end
      p_cur = p_cur.tail
      a_cur = a_cur.tail
    end
    new_env
  end
end

class Env
  attr_accessor :parent
  def initialize(p)
    @parent = p
    @names = []
    @vals = []
  end
  def define(n, v)
    @names.push(n)
    @vals.push(v)
  end
  def lookup(n)
    i = 0
    while i < @names.length
      if @names[i] == n
        return @vals[i]
      end
      i = i + 1
    end
    if @parent != nil
      return @parent.lookup(n)
    end
    nil
  end
end

class Reader
 # Tokenizes + parses in one class so the tokens array stays an
 # ivar of a single type. Splitting Tokenizer and Parser widened
 # Parser#@toks to int_array because constructor-arg inference
 # didn't carry the str_array shape across the new (toks) call.
  def initialize(src)
    @src = src
    @sp = 0
    @toks = []
    @tp = 0
  end
  def tokenize
    while @sp < @src.length
      c = @src[@sp]
      if c == " " || c == "\n" || c == "\t"
        @sp = @sp + 1
      elsif c == "("
        @toks.push("(")
        @sp = @sp + 1
      elsif c == ")"
        @toks.push(")")
        @sp = @sp + 1
      elsif c >= "0" && c <= "9"
        s = @sp
        while @sp < @src.length && @src[@sp] >= "0" && @src[@sp] <= "9"
          @sp = @sp + 1
        end
        @toks.push(@src[s, @sp - s])
      else
        s = @sp
        while @sp < @src.length && @src[@sp] != " " && @src[@sp] != "\n" && @src[@sp] != "\t" && @src[@sp] != "(" && @src[@sp] != ")"
          @sp = @sp + 1
        end
        @toks.push(@src[s, @sp - s])
      end
    end
  end
  def more?
    @tp < @toks.length
  end
  def parse
    t = @toks[@tp]
    @tp = @tp + 1
    if t == "("
      return parse_list
    end
    if t[0] >= "0" && t[0] <= "9"
      return IntV.new(t.to_i)
    end
    SymV.new(t)
  end
  def parse_list
    if @toks[@tp] == ")"
      @tp = @tp + 1
      return nil
    end
    h = parse
    t = parse_list
    PairV.new(h, t)
  end
end

def is_truthy(v)
  if v == nil
    return false
  end
  true
end

def int_of(v)
  if v.is_a?(IntV)
    return v.n
  end
  0
end

def list_get(lst, i)
  cur = lst
  k = 0
  while cur != nil && cur.is_a?(PairV) && k < i
    cur = cur.tail
    k = k + 1
  end
  if cur != nil && cur.is_a?(PairV)
    return cur.head
  end
  nil
end

def eval_form(form, env)
  if form == nil
    return nil
  end
  if form.is_a?(IntV)
    return form
  end
  if form.is_a?(SymV)
    return env.lookup(form.s)
  end
  if form.is_a?(PairV)
    h = form.head
    if h.is_a?(SymV)
      n = h.s
      if n == "quote"
        return form.tail.head
      end
      if n == "if"
        cond = eval_form(list_get(form, 1), env)
        if is_truthy(cond)
          return eval_form(list_get(form, 2), env)
        end
        return eval_form(list_get(form, 3), env)
      end
      if n == "define"
        name_sym = list_get(form, 1)
        val = eval_form(list_get(form, 2), env)
 # Hoist .s into a typed local. Without this hoist, spinel widens
 # Env.define's `n` param to poly because the is_a?(SymV) narrow
 # on name_sym doesn't propagate through the .s read at the call-
 # site arg position — name_sym.s on a poly recv reads as poly.
        if name_sym.is_a?(SymV)
          name_str = name_sym.s
          env.define(name_str, val)
        end
        return nil
      end
      if n == "lambda"
        params_form = list_get(form, 1)
        body = list_get(form, 2)
        return LambdaV.new(params_form, body, env)
      end
    end
    return apply_form(h, form.tail, env)
  end
  form
end

def eval_args(args_form, env)
  if args_form == nil
    return nil
  end
  if args_form.is_a?(PairV)
    hv = eval_form(args_form.head, env)
    tv = eval_args(args_form.tail, env)
    return PairV.new(hv, tv)
  end
  nil
end

def apply_form(head_form, args_form, env)
  evald_args = eval_args(args_form, env)
  if head_form.is_a?(SymV)
    bn = head_form.s
    if bn == "+"
      return IntV.new(int_of(list_get(evald_args, 0)) + int_of(list_get(evald_args, 1)))
    end
    if bn == "-"
      return IntV.new(int_of(list_get(evald_args, 0)) - int_of(list_get(evald_args, 1)))
    end
    if bn == "*"
      return IntV.new(int_of(list_get(evald_args, 0)) * int_of(list_get(evald_args, 1)))
    end
    if bn == "<"
      a = int_of(list_get(evald_args, 0))
      b = int_of(list_get(evald_args, 1))
      if a < b
        return IntV.new(1)
      end
      return nil
    end
  end
  fn = eval_form(head_form, env)
  if fn.is_a?(LambdaV)
    new_env = fn.bind(evald_args)
    return eval_form(fn.body, new_env)
  end
  nil
end

SRC = "(define fact (lambda (n) (if (< n 2) 1 (* n (fact (- n 1)))))) (fact 6)"

reader = Reader.new(SRC)
reader.tokenize
env = Env.new(nil)
result = nil
while reader.more?
  result = eval_form(reader.parse, env)
end
if result.is_a?(IntV)
  puts result.n
end
