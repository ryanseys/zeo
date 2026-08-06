require "prettyprint"

out = PrettyPrint.format("".dup, 20) do |q|
  q.group do
    q.text "["
    q.nest(2) do
      q.breakable ""
      %w[alpha beta gamma delta].each_with_index do |w, i|
        unless i.zero?
          q.text ","
          q.breakable
        end
        q.text w
      end
    end
    q.breakable ""
    q.text "]"
  end
end
puts out

wide = PrettyPrint.format("".dup, 80) do |q|
  q.group { q.text "a"; q.breakable; q.text "b" }
end
p wide

p PrettyPrint.singleline_format("".dup) { |q|
  q.group { q.text "x"; q.breakable; q.text "y" }
}
