# A present-but-lying to_x raises CRuby's "can't convert" shape; a nil
# answer from to_x means "not convertible" at check sites.
class LiarInt
  def to_int
    "nope"
  end
end

class LiarStr
  def to_str
    42
  end
end

class LiarAry
  def to_ary
    "zap"
  end
end

def err
  yield
rescue => e
  puts "#{e.class}: #{e.message}"
end

err { [1, 2, 3][LiarInt.new] }
err { [1, 2].first(LiarInt.new) }
err { "a" + LiarStr.new }
err { [1, 2].join(LiarStr.new) }
err { [1] + LiarAry.new }
err { [1].concat(LiarAry.new) }
err { "ab" * LiarInt.new }
__END__
TypeError: can't convert LiarInt to Integer (LiarInt#to_int gives String)
TypeError: can't convert LiarInt to Integer (LiarInt#to_int gives String)
TypeError: can't convert LiarStr to String (LiarStr#to_str gives Integer)
TypeError: can't convert LiarStr to String (LiarStr#to_str gives Integer)
TypeError: can't convert LiarAry to Array (LiarAry#to_ary gives String)
TypeError: can't convert LiarAry to Array (LiarAry#to_ary gives String)
TypeError: can't convert LiarInt to Integer (LiarInt#to_int gives String)
