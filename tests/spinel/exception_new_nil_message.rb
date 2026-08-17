# `Exception.new(nil)`: nil is not a message, so the class name answers, the
# way the no-argument form does; an empty String is a message (#3812).
p RuntimeError.new(nil).message
p KeyError.new(nil).message
p StandardError.new(nil).to_s
p RuntimeError.new("").message
p RuntimeError.new.message
p RuntimeError.new("real").message
class MyErr < StandardError; end
p MyErr.new(nil).message
p MyErr.new("m").message
begin
  raise RuntimeError, nil
rescue => e
  p e.message
end
