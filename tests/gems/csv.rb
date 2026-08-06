# csv vendored. Its parser is `class Scanner < StringScanner` and its rows use
# `forwardable`, whose delegators are generated with `eval` -- so this exercises
# native subclassing, `caller_locations` objects, and `(...)` forwarding in the
# eval VM all at once.
#
# (`CSV#shift` is still a gap -- see `tests/gaps/issue_csv_shift_nonlocal.rb`.)
require "csv"

p CSV.parse("a,b,c\n1,2,3\n")
p CSV.parse_line("x,y,z")
p CSV.generate_line(["a", "b,c", nil])
p CSV.new("x,y").read

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

p CSV::VERSION.is_a?(String)

begin
  CSV.parse_line("\"unclosed")
rescue CSV::MalformedCSVError => e
  puts e.class
end
