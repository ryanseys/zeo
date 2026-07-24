def tok(expr)
  tokens = []
  expr.scan(/\s*(\d+\.\d+|\d+|[+\-*\/()]|[a-z]+)/) { |m| tokens << m[0] }
  tokens
end
["3 + 4 * 2", "(x + 12) / y", "3.14 * r", "1+2"].each do |e|
  toks = tok(e)
  kinds = toks.map do |t|
    if t =~ /\A\d/ then "NUM"
    elsif t =~ /\A[a-z]/ then "ID"
    else "OP" end
  end
  puts kinds.join(",")
end
