# Phase 1A: bare `raise` (re-raise) inside a def's rescue clause.
# Pre-fix the codegen routed the surrounding begin/rescue's body
# through compile_body_into which uses compile_expr for the last
# stmt; compile_expr's raise arm ignored the 2-arg form and emitted
# `sp_raise(<class_expr>)` -- passing an sp_Class compound literal to
# sp_raise(const char *), C compile fail. Fix: expr-form raise now
# mirrors compile_control_call_stmt's 2-arg handling
# (sp_raise_cls("ClassName", msg)).

def helper
  begin
    raise StandardError, "inner"
  rescue => e
    raise
  end
end

begin
  helper
rescue => e
  puts "outer: #{e.class}: #{e.message}"
end

# Variant: 2-arg raise as the only statement (no rescue, propagates).
def thrower
  raise ArgumentError, "from thrower"
end

begin
  thrower
rescue ArgumentError => e
  puts "caught: #{e.message}"
end

# Variant: 1-arg raise (string only) in def + outer rescue.
def thrower2
  raise "plain message"
end

begin
  thrower2
rescue => e
  puts "got: #{e.class}: #{e.message}"
end
