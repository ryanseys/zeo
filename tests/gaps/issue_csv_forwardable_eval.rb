# csv vendors and its StringScanner-subclassing parser compiles, but loading it
# still dies in `forwardable`: `CSV::Row` uses `def_delegators`, which builds a
# method with `eval("proc do\n  def m(...)\n ... end\nend")` and `module_eval`s
# it. The eval-VM rejects `(...)` argument forwarding inside that proc --
# `unexpected 'do', ignoring it` -- so no delegator is ever defined.
require "csv"

p CSV.parse("a,b,c\n1,2,3\n")
p CSV.parse_line("x,y,z")
p CSV.generate_line(["a", "b,c", nil])

table = CSV.parse("name,age\nzeo,1\nruby,32\n", headers: true)
p table.headers
p table.map { |row| row["name"] }
p table["age"]
p table.first.to_h

p CSV.parse("a;b\n1;2\n", col_sep: ";")
p CSV.parse("a,b\n\"q,uoted\",2\n")

out = CSV.generate do |csv|
  csv << ["h1", "h2"]
  csv << [1, "two, three"]
end
p out

p CSV.new("a,b\n").shift
p CSV::VERSION.is_a?(String)

begin
  CSV.parse_line("\"unclosed")
rescue CSV::MalformedCSVError => e
  puts e.class
end
