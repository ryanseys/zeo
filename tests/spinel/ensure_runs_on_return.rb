# `begin..ensure..end` must run the ensure clause when the body
# exits via `return` (bare or with a value). Previously the
# generated C placed the ensure body *after* the unconditional
# `return` it followed, so the writeback was dead code.

class C
  def initialize
    @x = 0
    @v = 0
    @a = 0
  end

  def bare_return
    begin
      x = 100
      return
    ensure
      @x = x
    end
  end

  def value_return
    begin
      v = 7
      return v * 6
    ensure
      @v = v
    end
  end

  def multi_return(c)
    begin
      a = 1
      if c == 1
        return 11
      end
      a = 2
      if c == 2
        return 22
      end
      a = 3
    ensure
      @a = a
    end
  end

  def report
    bare_return
    puts value_return
    multi_return(1)
    puts "a1=" + @a.to_s
    multi_return(2)
    puts "a2=" + @a.to_s
    multi_return(3)
    puts "a3=" + @a.to_s
    puts @x
    puts @v
  end
end

C.new.report
