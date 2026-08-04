# An exception raised inside an `ensure` while another is propagating takes the
# propagating one as its `cause`. zeo leaves `cause` nil, so the original
# failure is gone.
#
# Ruby sets `cause` from whatever is in flight at the raise -- `$!` -- and an
# `ensure` running during propagation has the original in `$!`. That is the one
# case where the cause chain matters most: the ensure's own failure (a close
# that fails, a rollback that fails) is what surfaces, and the cause is the only
# remaining record of what went wrong first.
#
# zeo already builds the chain for the ordinary shape -- a raise inside a
# `rescue` gets its cause (the second block below) -- so `$!` is tracked and the
# ensure path simply does not consult it.

begin
  begin
    raise "first"
  ensure
    raise "second"
  end
rescue => e
  p [e.message, e.cause&.message]
end

# Already correct: raising from a rescue.
begin
  begin
    raise "orig"
  rescue
    raise "wrapped"
  end
rescue => e
  p [e.message, e.cause&.message]
end

# Three deep, through an ensure in the middle.
begin
  begin
    begin
      raise "a"
    ensure
      raise "b"
    end
  rescue
    raise "c"
  end
rescue => e
  p [e.message, e.cause&.message, e.cause&.cause&.message]
end

# An ensure that raises with NOTHING in flight has no cause in ruby either.
begin
  begin
    :fine
  ensure
    raise "alone"
  end
rescue => e
  p [e.message, e.cause&.message]
end
