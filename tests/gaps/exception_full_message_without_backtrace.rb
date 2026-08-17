e = StandardError.new("msg")
p e.backtrace
p e.full_message(highlight: false, order: :top)
