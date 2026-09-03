# `raise E` must CONSTRUCT via `E.new` -- running defaults and `super`
# -- rather than short-cutting the message to the class name. The
# class-name default lives in the prelude's `Exception#to_s`.

class E < StandardError
  def initialize(m = "def")
    super
  end
end
class F < StandardError
  def initialize(m = "dd")
    super("wrapped: #{m}")
  end
end
class G < StandardError; end

begin; raise E; rescue => e; p e.message; end
begin; raise E, "x"; rescue => e; p e.message; end
begin; raise E.new; rescue => e; p e.message; end
begin; raise F; rescue => e; p e.message; end
begin; raise G; rescue => e; p e.message; end
begin; raise StandardError; rescue => e; p e.message; end
__END__
"def"
"x"
"def"
"wrapped: dd"
"G"
"StandardError"
