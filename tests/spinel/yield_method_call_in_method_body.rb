# Calling a yield-using method on implicit self from inside another
# instance method's body used to drop the literal block — the call
# site emitted `sp_C_run(self, "foo")` against a signature that
# wanted 4 args. Same call from top-level worked.
#
# scan path: compile_writer_and_block_call_stmt's literal-block
# branch only handled `recv < 0` for top-level methods (via the
# global @meth_has_yield table) and `recv >= 0` for class methods
# (via try_yield_or_trampoline_dispatch). The "implicit self inside
# class method" case fell into neither.

class C
  def run(name)
    yield
    name
  end

  def call_it
    run("foo") do
      puts "block ran"
    end
  end

  # Variant: default arg in target. Same root cause; just a different
  # arity surface — exercised explicitly because the issue called it
  # out as a separate failure mode.
  def run_with_default(name, count = 1)
    yield
    count
  end

  def call_it_default
    run_with_default("bar", -1) do
      puts "default-arg block ran"
    end
  end
end

# Parent class with the yield method, child calls it via implicit
# self — exercises the new ancestor walk.
class P
  def each_thing
    yield 1
    yield 2
  end
end

class K < P
  def use
    each_thing do |v|
      puts v
    end
  end
end

C.new.call_it
C.new.call_it_default
K.new.use
